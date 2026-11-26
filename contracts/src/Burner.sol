// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @notice A launch's bonding curve, as far as a buy goes. It stops selling once the launch graduates.
interface ICurveBuy {
    function buy(uint256 quoteIn, uint256 minTokensOut, address recipient) external payable returns (uint256);
}

/// @notice The launch's token: a plain ERC-20 whose holders can burn what they hold.
interface IBurnableToken {
    function balanceOf(address account) external view returns (uint256);
    function totalSupply() external view returns (uint256);
    function burn(uint256 amount) external;
}

/// @notice The launchpad's pool hook. It registers a launch's pool when the curve graduates. Only the first three
///         fields of its record are read here.
interface ILaunchRegistry {
    function launches(bytes32 poolId)
        external
        view
        returns (bool registered, bool memecoinIsCurrency0, address memecoin);
}

/// @notice A Uniswap v4 pool, as the pool manager names it.
struct PoolKey {
    address currency0;
    address currency1;
    uint24 fee;
    int24 tickSpacing;
    address hooks;
}

/// @notice Uniswap v4's pool manager, as far as one exact-input swap goes.
interface IPoolManager {
    struct SwapParams {
        bool zeroForOne;
        int256 amountSpecified;
        uint160 sqrtPriceLimitX96;
    }

    function unlock(bytes calldata data) external returns (bytes memory);
    function swap(PoolKey memory key, SwapParams memory params, bytes calldata hookData) external returns (int256);
    function settle() external payable returns (uint256);
    function take(address currency, address to, uint256 amount) external;
}

/// @title Burner
/// @notice Turns the ETH it receives into the launch's token and burns it: on the bonding curve until the launch
///         graduates, then in the launch's Uniswap v4 pool.
/// @dev The ETH can go nowhere else. The only calls that move it are the curve's buy and the pool manager's settle,
///      and everything bought is burned in the same call.
///      Only the keeper picks the moment and the fewest tokens it accepts, because a burn with no floor could be
///      wrapped in trades that move the price first. If the keeper is silent for FALLBACK_AFTER, anyone may burn, at
///      most FALLBACK_MAX at a time, so the ETH never waits on one machine and no single call is worth moving the
///      price for.
contract Burner {
    IPoolManager public constant MANAGER = IPoolManager(0x8366a39CC670B4001A1121B8F6A443A643e40951);
    ILaunchRegistry public constant HOOK = ILaunchRegistry(0xE5e702641Ea86F4ae6cC3cDaeD2B886f976Be044);
    /// @dev Uniswap's lowest price plus one, so a buy may move the price as far as the pool allows. minOut is the
    ///      floor instead.
    uint160 private constant NO_PRICE_LIMIT = 4_295_128_740;
    /// @notice How long the keeper may be silent before anyone may burn.
    uint256 public constant FALLBACK_AFTER = 7 days;
    /// @notice The most ETH one burn by anyone but the keeper spends.
    uint256 public constant FALLBACK_MAX = 0.1 ether;

    /// @notice The machine that burns on a schedule.
    address public immutable keeper;
    /// @notice Who names the launch once it exists.
    address public immutable binder;
    /// @notice The token bought and burned.
    IBurnableToken public token;
    /// @notice The token's bonding curve.
    ICurveBuy public curve;
    /// @notice When the keeper last burned, or this burner was made.
    uint256 public lastKeeperBurn;
    PoolKey internal key;
    /// @dev Set while a burn runs.
    bool private entered;

    event Bound(address indexed token, address curve, bytes32 poolId);
    event Burned(uint256 ethIn, uint256 tokensBurned, bool onCurve);

    error ZeroAddress();
    error NotBinder();
    error AlreadyBound();
    error NotKeeper();
    error NotManager();
    error TooLittle();
    error Reentered();

    /// @dev A burn calls out to the launchpad or the pool manager. Nothing they call back may start a second burn.
    modifier nonReentrant() {
        if (entered) revert Reentered();
        entered = true;
        _;
        entered = false;
    }

    constructor(address keeper_) {
        if (keeper_ == address(0)) revert ZeroAddress();
        keeper = keeper_;
        binder = msg.sender;
        lastKeeperBurn = block.timestamp;
    }

    /// @notice Accepts the burn share. Kept empty so a sender's small gas stipend is enough.
    receive() external payable {}

    /// @notice Names the launch. The token exists only after the launch, the launch needs the splitter, and the
    ///         splitter needs this burner, so the naming comes last.
    /// @dev The token and curve are set once. The pool's fee and tick spacing may be named again, but only while the
    ///      pool they name is not the launch's registered pool, so a mistake can be fixed before it strands ETH, and
    ///      the right pool, once live, can never be swapped for another.
    function bind(IBurnableToken token_, ICurveBuy curve_, uint24 fee, int24 tickSpacing) external {
        if (msg.sender != binder) revert NotBinder();
        if (address(token) == address(0)) {
            if (address(token_) == address(0) || address(curve_) == address(0)) revert ZeroAddress();
            token = token_;
            curve = curve_;
        } else if (address(token_) != address(token) || address(curve_) != address(curve) || graduated()) {
            revert AlreadyBound();
        }
        // ETH is address zero, so it is always the pool's first currency.
        key = PoolKey(address(0), address(token_), fee, tickSpacing, address(HOOK));
        emit Bound(address(token_), address(curve_), poolId());
    }

    /// @notice The id of the pool the launch graduates into.
    function poolId() public view returns (bytes32) {
        return keccak256(abi.encode(key));
    }

    /// @notice Whether the launch has left its curve for its pool.
    function graduated() public view returns (bool) {
        // Only two fields of the hook's record matter here.
        // slither-disable-start unused-return
        // forge-lint: disable-next-line(unused-return)
        (bool registered,, address memecoin) = HOOK.launches(poolId());
        // slither-disable-end unused-return
        return registered && memecoin == address(token);
    }

    /// @notice Buys the token with the ETH held here and burns every token held here. Reverts if the buy brings back
    ///         fewer than minOut tokens.
    // The guard's own flag is the only state written after the calls out, and the balance read before the buy is
    // compared after it on purpose: that difference is what the buy brought.
    // forge-lint: disable-next-item(reentrancy-no-eth)
    // slither-disable-next-line reentrancy-balance
    function burn(uint256 minOut) external nonReentrant returns (uint256 burned) {
        uint256 amountIn = address(this).balance;
        if (msg.sender == keeper) {
            lastKeeperBurn = block.timestamp;
        } else {
            // A week is coarse enough that a validator's few seconds of clock leeway change nothing.
            // forge-lint: disable-next-line(block-timestamp)
            if (block.timestamp < lastKeeperBurn + FALLBACK_AFTER) revert NotKeeper();
            if (amountIn > FALLBACK_MAX) amountIn = FALLBACK_MAX;
        }
        IBurnableToken token_ = token;
        // Nothing to spend, or no launch named yet: nothing to do.
        // slither-disable-next-line incorrect-equality
        if (amountIn == 0 || address(token_) == address(0)) return 0;

        // What the buy brought is read from the balance before and after, not from what the buy reports.
        uint256 before = token_.balanceOf(address(this));
        bool onCurve = !graduated();
        // slither-disable-start unused-return
        if (onCurve) {
            // The curve is the launch's own, named once by the binder.
            // forge-lint: disable-next-line(arbitrary-send-eth)
            curve.buy{value: amountIn}(amountIn, 0, address(this));
        } else {
            // forge-lint: disable-next-line(unused-return)
            MANAGER.unlock(abi.encode(amountIn));
        }
        // slither-disable-end unused-return
        burned = token_.balanceOf(address(this));
        if (burned - before < minOut) revert TooLittle();
        token_.burn(burned);
        // forge-lint: disable-next-line(reentrancy-events)
        emit Burned(amountIn, burned, onCurve);
    }

    /// @dev The pool manager calls back here inside unlock: swap the ETH for the token, pay the ETH, take the token.
    function unlockCallback(bytes calldata data) external returns (bytes memory) {
        if (msg.sender != address(MANAGER)) revert NotManager();
        uint256 amountIn = abi.decode(data, (uint256));
        PoolKey memory poolKey = key;
        // The casts follow the pool manager's own packing: amountIn is at most this contract's balance, and each half
        // of the delta is an int128 by construction. A delta with the wrong signs makes settle or take revert.
        // forge-lint: disable-start(unsafe-typecast)
        int256 delta = MANAGER.swap(poolKey, IPoolManager.SwapParams(true, -int256(amountIn), NO_PRICE_LIMIT), "");
        // The delta packs this contract's side of the swap: the ETH it owes (negative) over the token it is owed.
        int128 ethDelta = int128(delta >> 128);
        int128 tokenDelta = int128(delta);
        // settle returns what was paid, which the delta already fixed.
        // slither-disable-next-line unused-return
        MANAGER.settle{value: uint256(uint128(-ethDelta))}();
        MANAGER.take(poolKey.currency1, address(this), uint256(uint128(tokenDelta)));
        // forge-lint: disable-end(unsafe-typecast)
        return "";
    }
}
