// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {BountyPool} from "../src/BountyPool.sol";

/// A breaker that tries to be paid twice by re-entering `execute`.
contract Reenterer {
    BountyPool internal immutable pool;
    uint256 internal id;
    bool internal tried;
    bool public reentryFailed;

    constructor(BountyPool pool_) {
        pool = pool_;
    }

    function arm(uint256 id_) external {
        id = id_;
    }

    receive() external payable {
        if (tried) return;
        tried = true;
        try pool.execute(id) {}
        catch {
            reentryFailed = true;
        }
    }
}

/// A breaker that refuses ETH.
contract Refuser {
    receive() external payable {
        revert("no");
    }
}

contract BountyPoolTest is Test {
    address internal constant JUDGE = address(0x7D9E);
    address payable internal constant TEAM = payable(address(0x7EA4));
    address internal constant BREAKER = address(0xB4EA);
    bytes32 internal constant CLAIM = keccak256("0x5a2b80a9b7effc06129bd5462d77bc20a8a59be7:V2_sweep");
    BountyPool internal pool;

    function setUp() public {
        pool = new BountyPool(JUDGE, TEAM);
        vm.deal(address(pool), 100 ether);
    }

    function propose(address breaker, uint256 amount, bytes32 claim) internal returns (uint256) {
        vm.prank(JUDGE);
        return pool.propose(breaker, amount, claim, "ipfs://evidence");
    }

    function test_onlyTheJudgeProposesAndCancels(address caller) public {
        vm.assume(caller != JUDGE);
        vm.prank(caller);
        vm.expectRevert(BountyPool.NotJudge.selector);
        pool.propose(BREAKER, 1 ether, CLAIM, "");
        uint256 id = propose(BREAKER, 1 ether, CLAIM);
        vm.prank(caller);
        vm.expectRevert(BountyPool.NotJudge.selector);
        pool.cancel(id);
    }

    function test_aBountyWaitsThenAnyoneCanPayIt() public {
        uint256 id = propose(BREAKER, 10 ether, CLAIM);
        vm.expectRevert(BountyPool.TooEarly.selector);
        pool.execute(id);
        vm.warp(block.timestamp + pool.DELAY());
        vm.prank(address(0xA11));
        pool.execute(id);
        assertEq(BREAKER.balance, 10 ether);
        assertEq(address(pool).balance, 90 ether);
        assertEq(pool.reserved(), 0);
        vm.expectRevert(BountyPool.NotPending.selector);
        pool.execute(id);
    }

    function test_aClaimIsPaidOnceAndCancellingFreesIt() public {
        uint256 id = propose(BREAKER, 1 ether, CLAIM);
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.ClaimTaken.selector);
        pool.propose(BREAKER, 1 ether, CLAIM, "");
        vm.prank(JUDGE);
        pool.cancel(id);
        assertEq(pool.reserved(), 0);
        assertFalse(pool.claimTaken(CLAIM));
        propose(BREAKER, 1 ether, CLAIM);
    }

    function test_oneBountyTakesAtMostAQuarterOfWhatIsUnpromised() public {
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.AmountOutOfRange.selector);
        pool.propose(BREAKER, 25 ether + 1, CLAIM, "");
        propose(BREAKER, 25 ether, CLAIM);
        // 75 ether is unpromised now, so the next cap is 18.75 ether.
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.AmountOutOfRange.selector);
        pool.propose(BREAKER, 18.75 ether + 1, keccak256("other"), "");
        propose(BREAKER, 18.75 ether, keccak256("other"));
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.AmountOutOfRange.selector);
        pool.propose(BREAKER, 0, keccak256("zero"), "");
    }

    function test_promisedEthIsAlwaysCovered(uint96[8] calldata amounts) public {
        for (uint256 i; i < amounts.length; ++i) {
            uint256 cap = pool.available() * pool.MAX_SHARE_BPS() / 10_000;
            uint256 amount = bound(amounts[i], 1, cap == 0 ? 1 : cap);
            if (cap == 0) break;
            propose(BREAKER, amount, keccak256(abi.encode(i)));
            assertLe(pool.reserved(), address(pool).balance);
        }
    }

    function test_reenteringExecutePaysOnce() public {
        Reenterer breaker = new Reenterer(pool);
        uint256 id = propose(address(breaker), 5 ether, CLAIM);
        breaker.arm(id);
        vm.warp(block.timestamp + pool.DELAY());
        pool.execute(id);
        assertTrue(breaker.reentryFailed());
        assertEq(address(breaker).balance, 5 ether);
    }

    function test_aBreakerThatRefusesEthLeavesTheBountyPending() public {
        Refuser breaker = new Refuser();
        uint256 id = propose(address(breaker), 5 ether, CLAIM);
        vm.warp(block.timestamp + pool.DELAY());
        vm.expectRevert(BountyPool.TransferFailed.selector);
        pool.execute(id);
        (,, BountyPool.State state,,) = pool.bounties(id);
        assertEq(uint8(state), uint8(BountyPool.State.Pending));
        assertEq(pool.reserved(), 5 ether);
    }

    function test_theJudgeAndTheTeamAreNeverZero() public {
        vm.expectRevert(BountyPool.ZeroAddress.selector);
        new BountyPool(address(0), TEAM);
        vm.expectRevert(BountyPool.ZeroAddress.selector);
        new BountyPool(JUDGE, payable(address(0)));
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.ZeroAddress.selector);
        pool.propose(address(0), 1 ether, CLAIM, "");
    }

    function closeNow() internal {
        vm.prank(JUDGE);
        pool.announceClose();
        vm.warp(vm.getBlockTimestamp() + pool.NOTICE());
    }

    function test_aCloseWaitsItsNoticeThenTheRestGoesToTheTeam() public {
        uint256 id = propose(BREAKER, 10 ether, CLAIM);
        vm.prank(JUDGE);
        pool.announceClose();
        vm.warp(vm.getBlockTimestamp() + pool.NOTICE() - 1);
        vm.expectRevert(BountyPool.NotClosed.selector);
        pool.sweep();
        // During the notice a bounty can still be proposed, and it is paid after the close.
        uint256 late = propose(BREAKER, 5 ether, keccak256("late"));
        vm.warp(vm.getBlockTimestamp() + 1);
        vm.prank(address(0xA11));
        pool.sweep();
        assertEq(TEAM.balance, 85 ether);
        assertEq(address(pool).balance, 15 ether);
        pool.execute(id);
        vm.expectRevert(BountyPool.TooEarly.selector);
        pool.execute(late);
        vm.warp(vm.getBlockTimestamp() + pool.DELAY());
        pool.execute(late);
        assertEq(BREAKER.balance, 15 ether);
        assertEq(address(pool).balance, 0);
    }

    function test_aClosedPoolSendsLaterEthToTheTeamToo() public {
        closeNow();
        pool.sweep();
        vm.deal(address(this), 3 ether);
        (bool ok,) = address(pool).call{value: 3 ether}("");
        assertTrue(ok);
        pool.sweep();
        assertEq(TEAM.balance, 103 ether);
        pool.sweep(); // nothing left: a no-op, not a revert
    }

    function test_aClosedPoolTakesNoNewBounty() public {
        closeNow();
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.Closed.selector);
        pool.propose(BREAKER, 1 ether, CLAIM, "");
    }

    function test_aCancelledBountyAfterTheCloseGoesToTheTeam() public {
        uint256 id = propose(BREAKER, 10 ether, CLAIM);
        closeNow();
        pool.sweep();
        assertEq(TEAM.balance, 90 ether);
        vm.prank(JUDGE);
        pool.cancel(id);
        pool.sweep();
        assertEq(TEAM.balance, 100 ether);
    }

    function test_theJudgeCanWithdrawACloseOnlyBeforeItTakesEffect() public {
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.NoCloseToWithdraw.selector);
        pool.withdrawClose();
        vm.prank(JUDGE);
        pool.announceClose();
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.CloseAlreadyAnnounced.selector);
        pool.announceClose();
        vm.prank(JUDGE);
        pool.withdrawClose();
        assertEq(pool.closesAt(), 0);
        vm.warp(vm.getBlockTimestamp() + pool.NOTICE());
        vm.expectRevert(BountyPool.NotClosed.selector);
        pool.sweep();
        closeNow();
        vm.prank(JUDGE);
        vm.expectRevert(BountyPool.NoCloseToWithdraw.selector);
        pool.withdrawClose();
    }

    function test_onlyTheJudgeAnnouncesOrWithdrawsAClose(address caller) public {
        vm.assume(caller != JUDGE);
        vm.prank(caller);
        vm.expectRevert(BountyPool.NotJudge.selector);
        pool.announceClose();
        vm.prank(JUDGE);
        pool.announceClose();
        vm.prank(caller);
        vm.expectRevert(BountyPool.NotJudge.selector);
        pool.withdrawClose();
    }
}
