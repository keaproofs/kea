// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @title BountyPool
/// @notice Pays people who break a claim Kea published as proven.
/// @dev ETH leaves this contract as a bounty, and every bounty is public before it moves:
///      - the judge (a multisig, fixed at deploy) proposes it with its evidence;
///      - anyone can pay it out once DELAY has passed, and the judge can cancel it before then;
///      - each claim is paid at most once;
///      - one bounty takes at most MAX_SHARE_BPS of the ETH not already promised to others.
///      The only other way out is a close, so a project that ends does not strand the pool:
///      - the judge announces it, and it takes effect NOTICE later, in public;
///      - until then bounties work as before, and every bounty proposed before then is still paid after;
///      - once closed, anyone can send the ETH no bounty is owed to the team address fixed at deploy.
///      There is no other withdrawal, no pause and no upgrade.
contract BountyPool {
    enum State {
        Pending,
        Cancelled,
        Paid
    }

    struct Bounty {
        address breaker;
        uint64 payableAt;
        State state;
        uint256 amount;
        bytes32 claim;
    }

    /// @notice How long a proposed bounty is public before it can be paid.
    uint256 public constant DELAY = 2 days;
    /// @notice How long an announced close is public before it takes effect.
    uint256 public constant NOTICE = 30 days;
    /// @notice The most one bounty can take, in basis points of the unpromised balance.
    uint256 public constant MAX_SHARE_BPS = 2_500;
    uint256 private constant BPS = 10_000;

    /// @notice Who decides that a claim was broken. Fixed for the life of the pool.
    address public immutable judge;
    /// @notice Where the ETH no bounty is owed goes once the pool has closed. Fixed for the life of the pool.
    address payable public immutable team;
    /// @notice When an announced close takes effect, or 0 while none is announced.
    uint64 public closesAt;
    /// @notice ETH promised to bounties that are proposed and not yet paid or cancelled.
    uint256 public reserved;
    /// @notice Whether a claim has a bounty pending or paid. A claim is
    ///         `keccak256(bytes("<contract address>:<claim id>"))`, as in Kea's reports.
    mapping(bytes32 claim => bool) public claimTaken;
    /// @notice Every bounty ever proposed, by id.
    Bounty[] public bounties;

    event Proposed(uint256 indexed id, address indexed breaker, uint256 amount, bytes32 indexed claim, string evidence);
    event Cancelled(uint256 indexed id);
    event Paid(uint256 indexed id, address indexed breaker, uint256 amount, bytes32 indexed claim);
    event CloseAnnounced(uint256 closesAt);
    event CloseWithdrawn();
    event Swept(uint256 amount);

    error NotJudge();
    error ZeroAddress();
    error AmountOutOfRange();
    error ClaimTaken();
    error NotPending();
    error TooEarly();
    error TransferFailed();
    error Closed();
    error NotClosed();
    error CloseAlreadyAnnounced();
    error NoCloseToWithdraw();

    constructor(address judge_, address payable team_) {
        if (judge_ == address(0) || team_ == address(0)) revert ZeroAddress();
        judge = judge_;
        team = team_;
    }

    /// @notice Anyone can fund the pool. Kept empty so a fee sender's small gas stipend is enough.
    receive() external payable {}

    /// @notice ETH a new bounty could draw on: the balance less what is already promised.
    function available() public view returns (uint256) {
        return address(this).balance - reserved;
    }

    /// @notice Whether an announced close has taken effect.
    function closed() public view returns (bool) {
        // forge-lint: disable-next-line(block-timestamp)
        return closesAt != 0 && block.timestamp >= closesAt;
    }

    /// @notice How many bounties have been proposed.
    function bountyCount() external view returns (uint256) {
        return bounties.length;
    }

    /// @notice Proposes a bounty for breaking `claim`, payable to `breaker` after DELAY.
    /// @param evidence Where the break is shown (a replay, a transaction, a write-up). Emitted, not stored.
    function propose(address breaker, uint256 amount, bytes32 claim, string calldata evidence)
        external
        returns (uint256 id)
    {
        if (msg.sender != judge) revert NotJudge();
        if (closed()) revert Closed();
        if (breaker == address(0)) revert ZeroAddress();
        if (claimTaken[claim]) revert ClaimTaken();
        if (amount == 0 || amount > available() * MAX_SHARE_BPS / BPS) revert AmountOutOfRange();

        claimTaken[claim] = true;
        reserved += amount;
        id = bounties.length;
        bounties.push(
            Bounty({
                breaker: breaker,
                // A timestamp two days ahead fits uint64 for the next 500 billion years.
                // forge-lint: disable-next-line(unsafe-typecast)
                payableAt: uint64(block.timestamp + DELAY),
                state: State.Pending,
                amount: amount,
                claim: claim
            })
        );
        emit Proposed(id, breaker, amount, claim, evidence);
    }

    /// @notice Withdraws a pending bounty before it is paid, freeing its claim and its ETH.
    function cancel(uint256 id) external {
        if (msg.sender != judge) revert NotJudge();
        Bounty storage bounty = bounties[id];
        if (bounty.state != State.Pending) revert NotPending();

        bounty.state = State.Cancelled;
        reserved -= bounty.amount;
        claimTaken[bounty.claim] = false;
        emit Cancelled(id);
    }

    /// @notice Pays a pending bounty once its delay has passed. Anyone can call it.
    function execute(uint256 id) external {
        Bounty storage bounty = bounties[id];
        if (bounty.state != State.Pending) revert NotPending();
        // The delay is two days; a validator's few seconds of drift cannot shorten it in any way that matters.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp < bounty.payableAt) revert TooEarly();

        bounty.state = State.Paid;
        reserved -= bounty.amount;
        emit Paid(id, bounty.breaker, bounty.amount, bounty.claim);

        // Paying the breaker is the point; only the judge names one, and it waits DELAY in public first.
        // forge-lint: disable-next-line(arbitrary-send-eth)
        (bool ok,) = bounty.breaker.call{value: bounty.amount}("");
        if (!ok) revert TransferFailed();
    }

    /// @notice Starts the public notice that ends the pool. Bounties keep working until it takes effect.
    function announceClose() external {
        if (msg.sender != judge) revert NotJudge();
        if (closesAt != 0) revert CloseAlreadyAnnounced();
        // A timestamp thirty days ahead fits uint64 for the next 500 billion years.
        // forge-lint: disable-next-line(unsafe-typecast)
        closesAt = uint64(block.timestamp + NOTICE);
        emit CloseAnnounced(closesAt);
    }

    /// @notice Takes back an announced close before it takes effect.
    function withdrawClose() external {
        if (msg.sender != judge) revert NotJudge();
        // Zero means no close was announced.
        // slither-disable-next-line incorrect-equality
        if (closesAt == 0 || closed()) revert NoCloseToWithdraw();
        closesAt = 0;
        emit CloseWithdrawn();
    }

    /// @notice Once the pool has closed, sends the ETH no bounty is owed to the team. Anyone can call it, as often as
    ///         ETH arrives; ETH promised to a pending bounty stays until that bounty is paid or cancelled.
    function sweep() external {
        if (!closed()) revert NotClosed();
        uint256 amount = available();
        // slither-disable-next-line incorrect-equality
        if (amount == 0) return;
        emit Swept(amount);
        (bool ok,) = team.call{value: amount}("");
        if (!ok) revert TransferFailed();
    }
}
