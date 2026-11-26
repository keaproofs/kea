// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @notice The launchpad's fee escrow: creator fees wait there until their recipient claims them.
interface IFeeEscrow {
    function balanceOf(address recipient) external view returns (uint256);
    function balanceOfToken(address recipient, address token) external view returns (uint256);
    function claim() external returns (uint256 amount);
    function claimToken(address token) external returns (uint256 amount);
}

/// @notice A launch's bonding curve. Its creator fees reach the escrow when the fee recipient sweeps them.
interface ILaunchCurve {
    function sweepFees(uint256 minBuybackTokensOut) external;
}

/// @notice The launchpad's pool hook after graduation, which holds the graduated pool's fees the same way.
interface ILaunchHook {
    function sweepPoolFees(bytes32 poolId, uint256 minConversionQuoteOut, uint256 minBuybackTokensOut) external;
}

/// @notice The two ERC-20 calls the splitter makes.
interface IERC20Balance {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
}

/// @title FeeSplitter
/// @notice Receives creator fees and splits them, at shares fixed at deploy, between the bounty pool, the burner (which
///         buys the token and burns it) and the team.
/// @dev Nobody can change the shares or the recipients after deploy, and the splitter can never hand its role as fee
///      recipient to anyone: it makes no call but the ones below. Anyone can move fees along and release them, so
///      none wait on one person.
///      Known limit: fees are split in ETH only. A launch must trade against ETH; any ERC-20 that reaches the splitter
///      (a launch paired with a token, or the launchpad's rescue path paying in the token itself) goes to the team
///      whole, since the pool pays bounties in ETH.
contract FeeSplitter {
    uint256 private constant BPS = 10_000;

    /// @notice Where the creator fees wait until they are claimed.
    IFeeEscrow public immutable escrow;
    /// @notice Where the bounty share goes.
    address payable public immutable pool;
    /// @notice Where the burn share goes.
    address payable public immutable burner;
    /// @notice Where the rest goes.
    address payable public immutable team;
    /// @notice The bounty share of every fee, in basis points.
    uint256 public immutable poolBps;
    /// @notice The burn share of every fee, in basis points.
    uint256 public immutable burnBps;
    /// @dev Set while a release runs.
    bool private entered;

    event Released(uint256 toPool, uint256 toBurner, uint256 toTeam);
    event TokenReleased(address indexed token, uint256 amount);

    error ZeroAddress();
    error ShareOutOfRange();
    error TransferFailed();
    error Reentered();

    /// @dev A release calls the escrow, the recipients or a token. Nothing they call back may start a second release.
    modifier nonReentrant() {
        if (entered) revert Reentered();
        entered = true;
        _;
        entered = false;
    }

    constructor(
        IFeeEscrow escrow_,
        address payable pool_,
        address payable burner_,
        address payable team_,
        uint256 poolBps_,
        uint256 burnBps_
    ) {
        if (address(escrow_) == address(0) || pool_ == address(0) || burner_ == address(0) || team_ == address(0)) {
            revert ZeroAddress();
        }
        if (poolBps_ + burnBps_ > BPS) revert ShareOutOfRange();
        escrow = escrow_;
        pool = pool_;
        burner = burner_;
        team = team_;
        poolBps = poolBps_;
        burnBps = burnBps_;
    }

    /// @notice Accepts fees. Kept empty so a fee sender's small gas stipend is enough.
    receive() external payable {}

    /// @notice Moves a launch curve's creator fees into the escrow. The launchpad lets only the fee recipient (this
    ///         splitter) or its own operator do that, so the splitter does it for whoever asks.
    /// @dev A target that is not a real curve can do nothing here but call this splitter back, which is harmless.
    function sweepCurve(ILaunchCurve curve) external {
        curve.sweepFees(0);
    }

    /// @notice The same for the graduated pool. Fees the pool took in the token itself must be converted first, which
    ///         the launchpad leaves to its operator; this moves the ones in ETH.
    function sweepPool(ILaunchHook hook, bytes32 poolId) external {
        hook.sweepPoolFees(poolId, 0, 0);
    }

    /// @notice Claims what the escrow holds for this splitter, then splits the whole balance between the pool, the
    ///         burner and the team. A failed send reverts the whole release, so nothing is lost and it can be retried.
    // The guard's own flag is the only state written after the calls out.
    // forge-lint: disable-next-item(reentrancy-no-eth, reentrancy-eth)
    function release() external nonReentrant {
        // The escrow reverts a claim of nothing, and ETH sent here directly must still be releasable. The claim's
        // amount goes unread, since the split is of the whole balance.
        // slither-disable-start unused-return
        // forge-lint: disable-next-line(incorrect-strict-equality, unused-return)
        if (escrow.balanceOf(address(this)) != 0) escrow.claim();
        // slither-disable-end unused-return
        uint256 balance = address(this).balance;
        // slither-disable-next-line incorrect-equality
        if (balance == 0) return;
        uint256 toPool = balance * poolBps / BPS;
        uint256 toBurner = balance * burnBps / BPS;
        uint256 toTeam = balance - toPool - toBurner;
        // forge-lint: disable-next-line(reentrancy-events)
        emit Released(toPool, toBurner, toTeam);

        (bool poolOk,) = pool.call{value: toPool}("");
        if (!poolOk) revert TransferFailed();
        (bool burnerOk,) = burner.call{value: toBurner}("");
        if (!burnerOk) revert TransferFailed();
        (bool teamOk,) = team.call{value: toTeam}("");
        if (!teamOk) revert TransferFailed();
    }

    /// @notice Claims an ERC-20 the escrow holds for this splitter, then sends every unit of it the splitter has to the
    ///         team. See the known limit above.
    // A zero token has no code, so the balance read below reverts. The guard keeps the token from calling back in.
    // forge-lint: disable-next-item(missing-zero-check, reentrancy-no-eth)
    // slither-disable-next-line reentrancy-balance
    function releaseToken(address token) external nonReentrant {
        // slither-disable-start unused-return
        // forge-lint: disable-next-line(unused-return)
        if (escrow.balanceOfToken(address(this), token) != 0) escrow.claimToken(token);
        // slither-disable-end unused-return
        uint256 amount = IERC20Balance(token).balanceOf(address(this));
        // slither-disable-next-line incorrect-equality
        if (amount == 0) return;
        // forge-lint: disable-next-line(reentrancy-events)
        emit TokenReleased(token, amount);
        // Tokens that return nothing from transfer are accepted, as long as the call did not fail.
        (bool ok, bytes memory data) = token.call(abi.encodeCall(IERC20Balance.transfer, (team, amount)));
        if (!ok || (data.length != 0 && !abi.decode(data, (bool)))) revert TransferFailed();
    }
}
