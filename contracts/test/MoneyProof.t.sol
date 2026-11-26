// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {SymTest} from "halmos-cheatcodes/SymTest.sol";
import {Burner} from "../src/Burner.sol";
import {FeeSplitter} from "../src/FeeSplitter.sol";
import {MockBurnToken, MockCurve, MockRegistry} from "./Burner.t.sol";
import {MockEscrow} from "./FeeSplitter.t.sol";

/// Kea's claims about its own burner, written the way Kea writes them for any contract.
/// Run with: halmos --contract BurnerProofTest --function check_
contract BurnerProofTest is Test, SymTest {
    address internal constant KEEPER = address(0x4EE9);
    Burner internal burner;
    MockBurnToken internal token;
    MockCurve internal curve;

    function setUp() public {
        burner = new Burner(KEEPER);
        vm.etch(address(burner.HOOK()), address(new MockRegistry()).code);
        token = new MockBurnToken();
        curve = new MockCurve(token);
        burner.bind(token, curve, 0, 60);
        vm.deal(address(burner), 10 ether);
    }

    /// While the keeper has burned within the week, nobody else can make the burner spend its ETH, by any function with
    /// any arguments.
    function check_onlyTheKeeperSpendsWithinTheWeek() public {
        address caller = svm.createAddress("caller");
        vm.assume(caller != KEEPER);
        uint256 elapsed = svm.createUint256("elapsed");
        vm.assume(elapsed < burner.FALLBACK_AFTER());
        vm.warp(block.timestamp + elapsed);
        bytes memory data = svm.createCalldata("Burner");
        uint256 before = address(burner).balance;
        vm.prank(caller);
        (bool ok,) = address(burner).call(data);
        ok;
        assert(address(burner).balance >= before);
    }

    /// Once the keeper has been silent for a week, anyone may burn, but one call spends at most FALLBACK_MAX.
    function check_aStrangerSpendsAtMostTheCap() public {
        address caller = svm.createAddress("caller");
        vm.assume(caller != KEEPER);
        uint256 elapsed = svm.createUint256("elapsed");
        vm.assume(elapsed >= burner.FALLBACK_AFTER() && elapsed < 100 * 365 days);
        vm.warp(block.timestamp + elapsed);
        uint256 minOut = svm.createUint256("minOut");
        uint256 before = address(burner).balance;
        vm.prank(caller);
        (bool ok,) = address(burner).call(abi.encodeCall(Burner.burn, (minOut)));
        ok;
        assert(before - address(burner).balance <= burner.FALLBACK_MAX());
    }

    /// A burn spends the ETH only on the curve's buy, and every token bought leaves the supply.
    function check_everyBurnBurnsAllItBought() public {
        uint256 minOut = svm.createUint256("minOut");
        uint256 supply = token.totalSupply();
        vm.prank(KEEPER);
        (bool ok,) = address(burner).call(abi.encodeCall(Burner.burn, (minOut)));
        if (ok) {
            assert(address(burner).balance == 0);
            assert(address(curve).balance == 10 ether);
            assert(token.balanceOf(address(burner)) == 0);
            assert(token.totalSupply() == supply);
        }
    }

    /// Once the launch's pool is live, nobody can point the burner at another pool, token or curve, the binder
    /// included, by any function with any arguments.
    function check_theLivePoolNeverChanges() public {
        bytes32 live = burner.poolId();
        MockRegistry(address(burner.HOOK())).register(live, address(token));
        address caller = svm.createAddress("caller");
        bytes memory data = svm.createCalldata("Burner");
        vm.prank(caller);
        (bool ok,) = address(burner).call(data);
        ok;
        assert(burner.poolId() == live);
        assert(address(burner.token()) == address(token));
        assert(address(burner.curve()) == address(curve));
    }
}

/// Kea's claims about its own fee splitter.
/// Run with: halmos --contract SplitterProofTest --function check_
contract SplitterProofTest is Test, SymTest {
    address payable internal constant POOL = payable(address(0xB0B0));
    address payable internal constant BURNER = payable(address(0xB124));
    address payable internal constant TEAM = payable(address(0x7EA4));
    FeeSplitter internal splitter;

    function setUp() public {
        splitter = new FeeSplitter(new MockEscrow(), POOL, BURNER, TEAM, 2_000, 3_000);
    }

    /// Whatever the splitter holds, a release pays exactly 20% to the pool, 30% to the burner and the rest to the team,
    /// and keeps nothing.
    function check_aReleasePaysExactlyTheShares() public {
        uint256 amount = svm.createUint256("amount");
        vm.assume(amount <= 1e12 ether);
        vm.deal(address(splitter), amount);
        splitter.release();
        assert(POOL.balance == amount * 2_000 / 10_000);
        assert(BURNER.balance == amount * 3_000 / 10_000);
        assert(POOL.balance + BURNER.balance + TEAM.balance == amount);
        assert(address(splitter).balance == 0);
    }

    /// Nobody can send the splitter's ETH anywhere but the pool, the burner and the team, by any function with any
    /// arguments.
    function check_feesReachOnlyTheThreeRecipients() public {
        vm.deal(address(splitter), 10 ether);
        address caller = svm.createAddress("caller");
        bytes memory data = svm.createCalldata("FeeSplitter");
        uint256 before = address(splitter).balance + POOL.balance + BURNER.balance + TEAM.balance;
        vm.prank(caller);
        (bool ok,) = address(splitter).call(data);
        ok;
        assert(address(splitter).balance + POOL.balance + BURNER.balance + TEAM.balance == before);
    }
}
