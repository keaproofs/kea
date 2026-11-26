// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {Burner, IBurnableToken, ICurveBuy} from "../src/Burner.sol";

/// A burnable ERC-20 with a public mint.
contract MockBurnToken is IBurnableToken {
    mapping(address account => uint256 amount) public balanceOf;
    uint256 public totalSupply;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
        totalSupply += amount;
    }

    function burn(uint256 amount) external {
        balanceOf[msg.sender] -= amount;
        totalSupply -= amount;
    }
}

/// A curve that sells 1,000 tokens per wei, as the real one sells at its own price.
contract MockCurve is ICurveBuy {
    MockBurnToken internal immutable token;

    constructor(MockBurnToken token_) {
        token = token_;
    }

    function buy(uint256 quoteIn, uint256, address recipient) external payable returns (uint256 out) {
        require(msg.value == quoteIn, "value");
        out = quoteIn * 1_000;
        token.mint(recipient, out);
    }
}

/// The launchpad hook's registry of graduated pools, placed at the hook's address.
contract MockRegistry {
    mapping(bytes32 poolId => address memecoin) public memecoinOf;

    function register(bytes32 poolId, address memecoin) external {
        memecoinOf[poolId] = memecoin;
    }

    function launches(bytes32 poolId) external view returns (bool, bool, address) {
        address memecoin = memecoinOf[poolId];
        return (memecoin != address(0), false, memecoin);
    }
}

/// A curve that tries to start a second burn from inside the first one's buy, and keeps what the try answered.
contract ReenteringCurve is ICurveBuy {
    MockBurnToken internal immutable token;
    bytes public answer;

    constructor(MockBurnToken token_) {
        token = token_;
    }

    function buy(uint256 quoteIn, uint256, address recipient) external payable returns (uint256) {
        (, answer) = msg.sender.call(abi.encodeCall(Burner.burn, (0)));
        token.mint(recipient, quoteIn);
        return quoteIn;
    }
}

contract BurnerTest is Test {
    address internal constant KEEPER = address(0x4EE9);
    address internal constant STRANGER = address(0x5742);
    Burner internal burner;
    MockBurnToken internal token;
    MockCurve internal curve;

    function setUp() public {
        burner = new Burner(KEEPER);
        vm.etch(address(burner.HOOK()), address(new MockRegistry()).code);
        token = new MockBurnToken();
        curve = new MockCurve(token);
        burner.bind(token, curve, 0, 60);
    }

    function test_onlyTheKeeperBurnsBeforeTheFallback() public {
        vm.deal(address(burner), 1 ether);
        vm.prank(STRANGER);
        vm.expectRevert(Burner.NotKeeper.selector);
        burner.burn(0);

        vm.expectEmit(address(burner));
        emit Burner.Burned(1 ether, 1_000 ether, true);
        vm.prank(KEEPER);
        assertEq(burner.burn(1_000 ether), 1_000 ether);
        // every wei bought tokens, and every token bought is gone
        assertEq(address(burner).balance, 0);
        assertEq(token.totalSupply(), 0);
    }

    function test_aBurnBelowTheFloorRevertsAndKeepsTheEth() public {
        vm.deal(address(burner), 1 ether);
        vm.prank(KEEPER);
        vm.expectRevert(Burner.TooLittle.selector);
        burner.burn(1_000 ether + 1);
        assertEq(address(burner).balance, 1 ether);
    }

    function test_tokensSentHereAreBurnedToo() public {
        token.mint(address(burner), 5);
        vm.deal(address(burner), 1);
        vm.prank(KEEPER);
        assertEq(burner.burn(1_000), 1_005);
        assertEq(token.totalSupply(), 0);
    }

    function test_anyoneBurnsInSmallStepsOnceTheKeeperIsSilent() public {
        vm.deal(address(burner), 1 ether);
        vm.warp(block.timestamp + burner.FALLBACK_AFTER());
        vm.prank(STRANGER);
        burner.burn(0);
        assertEq(address(burner).balance, 0.9 ether, "one step spends at most FALLBACK_MAX");
        vm.prank(STRANGER);
        burner.burn(0);
        assertEq(address(burner).balance, 0.8 ether, "the keeper's silence still counts");

        // the keeper comes back: the clock starts again, and so does the wait
        vm.prank(KEEPER);
        burner.burn(0);
        vm.deal(address(burner), 1 ether);
        vm.prank(STRANGER);
        vm.expectRevert(Burner.NotKeeper.selector);
        burner.burn(0);
    }

    function test_anUnboundBurnerHoldsTheEth() public {
        Burner unbound = new Burner(KEEPER);
        vm.deal(address(unbound), 1 ether);
        vm.prank(KEEPER);
        assertEq(unbound.burn(0), 0);
        assertEq(address(unbound).balance, 1 ether);
    }

    function test_theLaunchIsNamedOnceAndItsLivePoolNeverChanges() public {
        Burner fresh = new Burner(KEEPER);
        vm.prank(STRANGER);
        vm.expectRevert(Burner.NotBinder.selector);
        fresh.bind(token, curve, 0, 60);
        vm.expectRevert(Burner.ZeroAddress.selector);
        fresh.bind(IBurnableToken(address(0)), curve, 0, 60);
        vm.expectRevert(Burner.ZeroAddress.selector);
        new Burner(address(0));

        fresh.bind(token, curve, 0, 60);
        vm.expectRevert(Burner.AlreadyBound.selector);
        fresh.bind(IBurnableToken(address(0xBEEF)), curve, 0, 60);
        vm.expectRevert(Burner.AlreadyBound.selector);
        fresh.bind(token, ICurveBuy(address(0xBEEF)), 0, 60);

        // a wrong pool can be corrected while it is not the launch's live pool
        fresh.bind(token, curve, 3_000, 60);
        bytes32 live = fresh.poolId();
        MockRegistry(address(fresh.HOOK())).register(live, address(token));
        assertTrue(fresh.graduated());
        vm.expectRevert(Burner.AlreadyBound.selector);
        fresh.bind(token, curve, 0, 60);
        assertEq(fresh.poolId(), live);
    }

    function test_onlyThePoolManagerCallsBack() public {
        vm.prank(STRANGER);
        vm.expectRevert(Burner.NotManager.selector);
        burner.unlockCallback(abi.encode(uint256(1)));
    }

    function test_aBurnCannotStartAnotherFromInside() public {
        Burner fresh = new Burner(KEEPER);
        ReenteringCurve sly = new ReenteringCurve(token);
        fresh.bind(token, sly, 0, 60);
        vm.deal(address(fresh), 1 ether);
        // past the keeper's week, so the curve's own call gets past the keeper check and meets the guard
        vm.warp(block.timestamp + fresh.FALLBACK_AFTER());
        vm.prank(STRANGER);
        fresh.burn(0);
        assertEq(bytes4(sly.answer()), Burner.Reentered.selector);
        assertEq(address(fresh).balance, 0.9 ether, "the outer burn spent its one step, and only that");
    }
}
