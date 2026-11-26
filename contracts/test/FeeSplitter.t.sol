// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {FeeSplitter, IFeeEscrow, ILaunchCurve, ILaunchHook} from "../src/FeeSplitter.sol";

contract Refuser {
    receive() external payable {
        revert("no");
    }
}

/// A recipient that tries to start a second release from inside the first one's payment, and keeps the answer.
contract ReenteringRecipient {
    FeeSplitter public splitter;
    bytes public answer;

    function aim(FeeSplitter splitter_) external {
        splitter = splitter_;
    }

    receive() external payable {
        if (answer.length == 0) (, answer) = address(splitter).call(abi.encodeCall(FeeSplitter.release, ()));
    }
}

/// The launchpad escrow's behaviour that matters here: credits pile up, a claim pays all of them, a claim of
/// nothing reverts. The fork test runs the same flow against the real escrow.
contract MockEscrow is IFeeEscrow {
    mapping(address recipient => uint256 amount) public balanceOf;
    mapping(address recipient => mapping(address token => uint256 amount)) public balanceOfToken;

    function credit(address recipient) external payable {
        balanceOf[recipient] += msg.value;
    }

    function creditToken(address recipient, MockToken token, uint256 amount) external {
        token.mint(address(this), amount);
        balanceOfToken[recipient][address(token)] += amount;
    }

    function claimToken(address token) external returns (uint256 amount) {
        amount = balanceOfToken[msg.sender][token];
        require(amount != 0, "NoBalance");
        balanceOfToken[msg.sender][token] = 0;
        MockToken(token).transfer(msg.sender, amount);
    }

    function claim() external returns (uint256 amount) {
        amount = balanceOf[msg.sender];
        require(amount != 0, "NoBalance");
        balanceOf[msg.sender] = 0;
        (bool ok,) = msg.sender.call{value: amount}("");
        require(ok, "TransferFailed");
    }
}

/// A plain ERC-20 with a public mint.
contract MockToken {
    mapping(address account => uint256 amount) public balanceOf;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// A curve or hook stand-in that credits its pending fees to whoever sweeps, as the real ones credit the recipient.
contract MockLaunch is ILaunchCurve, ILaunchHook {
    MockEscrow internal immutable escrow;
    bytes32 public sweptPool;

    constructor(MockEscrow escrow_) payable {
        escrow = escrow_;
    }

    function sweepFees(uint256) external {
        escrow.credit{value: address(this).balance}(msg.sender);
    }

    function sweepPoolFees(bytes32 poolId, uint256, uint256) external {
        sweptPool = poolId;
        escrow.credit{value: address(this).balance}(msg.sender);
    }
}

contract FeeSplitterTest is Test {
    address payable internal constant POOL = payable(address(0xB0B0));
    address payable internal constant BURNER = payable(address(0xB124));
    address payable internal constant TEAM = payable(address(0x7EA4));
    MockEscrow internal escrow;

    function setUp() public {
        escrow = new MockEscrow();
    }

    function split(uint256 poolBps, uint256 burnBps) internal returns (FeeSplitter) {
        return new FeeSplitter(escrow, POOL, BURNER, TEAM, poolBps, burnBps);
    }

    function test_everyWeiGoesToThePoolTheBurnerOrTheTeam(
        uint16 poolBps,
        uint16 burnBps,
        uint128 credited,
        uint128 direct
    ) public {
        poolBps = uint16(bound(poolBps, 0, 10_000));
        burnBps = uint16(bound(burnBps, 0, 10_000 - poolBps));
        FeeSplitter splitter = split(poolBps, burnBps);
        vm.deal(address(this), credited);
        escrow.credit{value: credited}(address(splitter));
        vm.deal(address(splitter), direct);
        splitter.release();
        uint256 total = uint256(credited) + direct;
        assertEq(POOL.balance, total * poolBps / 10_000);
        assertEq(BURNER.balance, total * burnBps / 10_000);
        assertEq(POOL.balance + BURNER.balance + TEAM.balance, total);
        assertEq(address(splitter).balance, 0);
        assertEq(escrow.balanceOf(address(splitter)), 0);
    }

    function test_aRefusedSendKeepsEverythingForARetry() public {
        Refuser refuser = new Refuser();
        address payable refusing = payable(address(refuser));
        FeeSplitter[2] memory splitters = [
            new FeeSplitter(escrow, refusing, BURNER, TEAM, 2_000, 3_000),
            new FeeSplitter(escrow, POOL, refusing, TEAM, 2_000, 3_000)
        ];
        for (uint256 i; i < 2; ++i) {
            vm.deal(address(this), 1 ether);
            escrow.credit{value: 1 ether}(address(splitters[i]));
            vm.expectRevert(FeeSplitter.TransferFailed.selector);
            splitters[i].release();
            // the claim rolled back with the sends: the fees still wait in the escrow
            assertEq(escrow.balanceOf(address(splitters[i])), 1 ether);
            assertEq(address(splitters[i]).balance, 0);
        }
    }

    function test_theSharesAndRecipientsAreChecked() public {
        vm.expectRevert(FeeSplitter.ShareOutOfRange.selector);
        split(10_001, 0);
        vm.expectRevert(FeeSplitter.ShareOutOfRange.selector);
        split(5_000, 5_001);
        vm.expectRevert(FeeSplitter.ZeroAddress.selector);
        new FeeSplitter(escrow, payable(address(0)), BURNER, TEAM, 1, 1);
        vm.expectRevert(FeeSplitter.ZeroAddress.selector);
        new FeeSplitter(escrow, POOL, payable(address(0)), TEAM, 1, 1);
        vm.expectRevert(FeeSplitter.ZeroAddress.selector);
        new FeeSplitter(IFeeEscrow(address(0)), POOL, BURNER, TEAM, 1, 1);
    }

    function test_releasingNothingDoesNothing() public {
        FeeSplitter splitter = split(2_000, 3_000);
        splitter.release();
        assertEq(POOL.balance + BURNER.balance + TEAM.balance, 0);
    }

    function test_anyoneMovesCurveAndPoolFeesIntoTheEscrow() public {
        FeeSplitter splitter = split(2_000, 3_000);
        MockLaunch curve = new MockLaunch{value: 1 ether}(escrow);
        vm.prank(address(0xA11));
        splitter.sweepCurve(ILaunchCurve(address(curve)));
        MockLaunch hook = new MockLaunch{value: 2 ether}(escrow);
        vm.prank(address(0xA12));
        splitter.sweepPool(ILaunchHook(address(hook)), bytes32("pool"));
        assertEq(hook.sweptPool(), bytes32("pool"));
        assertEq(escrow.balanceOf(address(splitter)), 3 ether);
        splitter.release();
        assertEq(POOL.balance, 0.6 ether);
        assertEq(BURNER.balance, 0.9 ether);
        assertEq(TEAM.balance, 1.5 ether);
    }

    function test_anErc20GoesToTheTeamWhole() public {
        FeeSplitter splitter = split(2_000, 3_000);
        MockToken token = new MockToken();
        escrow.creditToken(address(splitter), token, 70);
        token.mint(address(splitter), 30); // paid directly, as the launchpad's rescue path does
        vm.prank(address(0xA11));
        splitter.releaseToken(address(token));
        assertEq(token.balanceOf(TEAM), 100);
        assertEq(token.balanceOf(POOL) + token.balanceOf(BURNER), 0);
        assertEq(escrow.balanceOfToken(address(splitter), address(token)), 0);
        splitter.releaseToken(address(token)); // nothing left: a no-op, not a revert
    }

    function test_aReleaseCannotStartAnotherFromInside() public {
        ReenteringRecipient sly = new ReenteringRecipient();
        FeeSplitter splitter = new FeeSplitter(escrow, payable(address(sly)), BURNER, TEAM, 2_000, 3_000);
        sly.aim(splitter);
        vm.deal(address(splitter), 1 ether);
        splitter.release();
        assertEq(bytes4(sly.answer()), FeeSplitter.Reentered.selector);
        assertEq(address(sly).balance + BURNER.balance + TEAM.balance, 1 ether, "the one release still paid all three");
    }
}
