// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {SymTest} from "halmos-cheatcodes/SymTest.sol";
import {BountyPool} from "../src/BountyPool.sol";

/// Kea's claims about its own bounty pool, written the way Kea writes them for any contract.
/// Run with: halmos --contract ProofTest --function check_
contract ProofTest is Test, SymTest {
    address internal constant JUDGE = address(0x7D9E);
    address payable internal constant TEAM = payable(address(0x7EA4));
    address internal constant BREAKER = address(0xB4EA);
    BountyPool internal pool;

    function setUp() public {
        pool = new BountyPool(JUDGE, TEAM);
        vm.deal(address(pool), 100 ether);
    }

    /// Nobody but the judge can propose a bounty.
    function check_onlyTheJudgeProposes() public {
        address caller = svm.createAddress("caller");
        vm.assume(caller != JUDGE);
        address breaker = svm.createAddress("breaker");
        uint256 amount = svm.createUint256("amount");
        bytes32 claim = svm.createBytes32("claim");
        vm.prank(caller);
        (bool ok,) = address(pool).call(abi.encodeCall(BountyPool.propose, (breaker, amount, claim, "")));
        assert(!ok);
    }

    /// With no bounty proposed and no close announced, nobody can take ETH out of the pool, by any function with any
    /// arguments.
    function check_nobodyTakesEthWithoutABounty() public {
        address caller = svm.createAddress("caller");
        bytes memory data = svm.createCalldata("BountyPool");
        uint256 before = address(pool).balance;
        vm.prank(caller);
        (bool ok,) = address(pool).call(data);
        ok;
        assert(address(pool).balance >= before);
    }

    /// A proposed bounty cannot be paid before its delay, whoever calls.
    function check_aBountyWaitsForItsDelay() public {
        vm.prank(JUDGE);
        uint256 id = pool.propose(address(0xB4EA), 1 ether, bytes32("claim"), "");
        uint256 elapsed = svm.createUint256("elapsed");
        vm.assume(elapsed < pool.DELAY());
        vm.warp(block.timestamp + elapsed);
        address caller = svm.createAddress("caller");
        vm.prank(caller);
        (bool ok,) = address(pool).call(abi.encodeCall(BountyPool.execute, (id)));
        assert(!ok);
    }

    /// No single bounty can promise more than a quarter of the unpromised ETH.
    function check_oneBountyTakesAtMostAQuarter() public {
        uint256 amount = svm.createUint256("amount");
        vm.prank(JUDGE);
        (bool ok,) =
            address(pool).call(abi.encodeCall(BountyPool.propose, (address(0xB4EA), amount, bytes32("claim"), "")));
        if (ok) assert(amount <= 25 ether);
    }

    /// A claim can hold one bounty at a time.
    function check_aClaimIsPaidOnce() public {
        bytes32 claim = svm.createBytes32("claim");
        vm.prank(JUDGE);
        pool.propose(address(0xB4EA), 1 ether, claim, "");
        uint256 amount = svm.createUint256("amount");
        vm.prank(JUDGE);
        (bool ok,) = address(pool).call(abi.encodeCall(BountyPool.propose, (address(0xB4EB), amount, claim, "")));
        assert(!ok);
    }

    /// After a bounty is proposed, any one call by anyone leaves the promised ETH covered by the balance.
    function check_promisedEthStaysCovered() public {
        uint256 amount = svm.createUint256("amount");
        vm.assume(amount > 0 && amount <= 25 ether);
        vm.prank(JUDGE);
        pool.propose(address(0xB4EA), amount, bytes32("claim"), "");
        address caller = svm.createAddress("caller");
        bytes memory data = svm.createCalldata("BountyPool");
        vm.prank(caller);
        (bool ok,) = address(pool).call(data);
        ok;
        assert(pool.reserved() <= address(pool).balance);
    }

    /// Nobody but the judge can start a close, by any function with any arguments.
    function check_onlyTheJudgeStartsAClose() public {
        address caller = svm.createAddress("caller");
        vm.assume(caller != JUDGE);
        bytes memory data = svm.createCalldata("BountyPool");
        vm.prank(caller);
        (bool ok,) = address(pool).call(data);
        ok;
        assert(pool.closesAt() == 0);
    }

    /// While an announced close has not taken effect, no call by anyone takes ETH out of a pool with no bounty.
    function check_nothingLeavesDuringTheNotice() public {
        vm.prank(JUDGE);
        pool.announceClose();
        uint256 elapsed = svm.createUint256("elapsed");
        vm.assume(elapsed < pool.NOTICE());
        vm.warp(block.timestamp + elapsed);
        address caller = svm.createAddress("caller");
        bytes memory data = svm.createCalldata("BountyPool");
        uint256 before = address(pool).balance;
        vm.prank(caller);
        (bool ok,) = address(pool).call(data);
        ok;
        assert(address(pool).balance >= before);
    }

    /// Once closed, any one call by anyone sends ETH only to the team or to a proposed bounty's breaker, and what is
    /// promised to the bounty stays covered.
    function check_aClosedPoolPaysOnlyTheTeamAndItsBounties() public {
        uint256 amount = svm.createUint256("amount");
        vm.assume(amount > 0 && amount <= 25 ether);
        vm.prank(JUDGE);
        pool.propose(BREAKER, amount, bytes32("claim"), "");
        vm.prank(JUDGE);
        pool.announceClose();
        vm.warp(block.timestamp + pool.NOTICE());
        address caller = svm.createAddress("caller");
        bytes memory data = svm.createCalldata("BountyPool");
        uint256 before = address(pool).balance;
        uint256 teamBefore = TEAM.balance;
        uint256 breakerBefore = BREAKER.balance;
        vm.prank(caller);
        (bool ok,) = address(pool).call(data);
        ok;
        assert(address(pool).balance + (TEAM.balance - teamBefore) + (BREAKER.balance - breakerBefore) == before);
        assert(pool.reserved() <= address(pool).balance);
    }
}
