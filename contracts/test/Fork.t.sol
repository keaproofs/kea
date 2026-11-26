// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {BountyPool} from "../src/BountyPool.sol";
import {Burner, IBurnableToken, ICurveBuy} from "../src/Burner.sol";
import {FeeSplitter, IFeeEscrow, ILaunchCurve} from "../src/FeeSplitter.sol";

/// The launchpad's factory on Robinhood Chain, as far as a launch touches it.
interface ILaunchFactory {
    struct Socials {
        string twitter;
        string telegram;
        string discord;
        string website;
        string farcaster;
    }

    struct TokenParams {
        string name;
        string symbol;
        string logo;
        string description;
        Socials socials;
        address creatorFeeRecipient;
        uint16 creatorTaxBps;
        bool buybackEnabled;
        bytes32 expectedEconomics;
        bytes32 salt;
    }

    struct LaunchConfig {
        uint256 supply;
        uint256 curveFeeBps;
        uint256 phantomQuote;
        uint256 graduationThreshold;
        uint24 poolFee;
        int24 tickSpacing;
        bool enabled;
    }

    function launchFee() external view returns (uint256);
    function launchConfigCount() external view returns (uint256);
    function getLaunchConfig(uint256 id) external view returns (LaunchConfig memory);
    function canLaunch(address launcher) external view returns (bool);
    function feeEscrow() external view returns (address);
    function launchToken(TokenParams calldata params, uint256 launchConfigId, address pairToken)
        external
        payable
        returns (address token, address curve);
}

/// A launch's bonding curve.
interface ICurveTrade {
    function buy(uint256 quoteIn, uint256 minTokensOut, address recipient) external payable returns (uint256);
    function graduated() external view returns (bool);
}

/// The whole money path on a copy of the live chain, against the launchpad's real contracts: a launch whose creator
/// fees go to the splitter, a trade, the fees claimed and split, the burn share burned, then the pool closed and
/// returned to the team. Runs only with RH_RPC set: RH_RPC=<robinhood chain rpc> forge test --match-contract ForkTest
contract ForkTest is Test {
    ILaunchFactory internal constant FACTORY = ILaunchFactory(0x7eD598BcEf8bd9Edd8C97A195C6d13f40801EC7e);
    IFeeEscrow internal constant ESCROW = IFeeEscrow(0xd3AFEB2a57f70eF218Aa82451c51B2fb0416Ac9e);
    /// A launch that graduated on 8 Nov 2026, and its live pool, standing in for $KEA after graduation.
    IBurnableToken internal constant TEXEL = IBurnableToken(0x0DdCAb9ae04bDA0c87eCC1640636FA11e8e613E8);
    bytes32 internal constant TEXEL_POOL = 0x5e73f5892ef5cf4d553fe0cc9b109e884c3c2a52635bf6c4f1dceff9637fe932;

    function test_feesTravelFromALaunchToThePoolTheBurnAndTheTeam() public {
        fork();
        address judge = makeAddr("judge");
        address keeper = makeAddr("keeper");
        address payable team = payable(makeAddr("team"));
        assertEq(FACTORY.feeEscrow(), address(ESCROW), "the factory pays creator fees into this escrow");
        assertTrue(FACTORY.canLaunch(makeAddr("launcher")), "launches are open to anyone");

        BountyPool pool = new BountyPool(judge, team);
        Burner burner = new Burner(keeper);
        FeeSplitter splitter =
            new FeeSplitter(ESCROW, payable(address(pool)), payable(address(burner)), team, 2_000, 3_000);

        // The team launches with the splitter as the creator fee recipient and a 2% creator tax, then names the
        // launch to the burner. Past the opening snipe tax, someone buys.
        (address token, address curve) = launch(address(splitter), 200);
        bindTo(burner, token, curve);
        buyOnce(curve);

        // Anyone has the splitter, as the fee recipient, move the curve's fees into the escrow.
        vm.prank(makeAddr("anyone"));
        splitter.sweepCurve(ILaunchCurve(curve));
        uint256 fees = ESCROW.balanceOf(address(splitter));
        assertGt(fees, 0, "the trade earned creator fees");

        // Anyone releases: the escrow pays the splitter, which pays 20% to the pool, 30% to the burner and the rest
        // to the team.
        vm.prank(makeAddr("anyone"));
        splitter.release();
        uint256 toBurner = fees * 3_000 / 10_000;
        assertEq(ESCROW.balanceOf(address(splitter)), 0);
        assertEq(address(pool).balance, fees * 2_000 / 10_000);
        assertEq(address(burner).balance, toBurner);
        assertEq(team.balance, fees - fees * 2_000 / 10_000 - toBurner);

        // The keeper burns: the burn share buys the token on the curve, and every token bought leaves the supply.
        assertBurns(burner, keeper, IBurnableToken(token));

        // The project ends: the close is announced, and after its notice the pool's ETH goes back to the team.
        vm.prank(judge);
        pool.announceClose();
        vm.warp(vm.getBlockTimestamp() + pool.NOTICE());
        pool.sweep();
        assertEq(address(pool).balance, 0);
        assertEq(team.balance, fees - toBurner);
    }

    /// A launch with a 2% creator tax: the whole tax must reach the splitter beside the 1% fee, or the tax would be
    /// money the bounty and the team never see.
    function test_creatorTaxReachesTheSplitter() public {
        fork();
        uint256 plain = feesFromOneBuy(0);
        uint256 taxed = feesFromOneBuy(200);
        emit log_named_uint("creator fees from a 1 ETH buy, no tax", plain);
        emit log_named_uint("creator fees from a 1 ETH buy, 2% tax", taxed);
        assertApproxEqRel(taxed - plain, 0.02 ether, 0.02e18, "the whole tax reaches the splitter");
    }

    /// After graduation the burner buys in the launch's Uniswap v4 pool. TEXEL's pool is found from the launchpad's own
    /// terms, and a burn takes TEXEL out of its supply.
    function test_theBurnerBurnsInAGraduatedPool() public {
        fork();
        address keeper = makeAddr("keeper");
        Burner burner = new Burner(keeper);
        bindTo(burner, address(TEXEL), address(1));
        assertEq(burner.poolId(), TEXEL_POOL, "the launchpad's terms name TEXEL's live pool");
        assertTrue(burner.graduated());

        vm.deal(address(burner), 0.05 ether);
        uint256 burned = assertBurns(burner, keeper, TEXEL);
        emit log_named_uint("TEXEL burned for 0.05 ETH", burned);
    }

    function fork() internal {
        string memory rpc = vm.envOr("RH_RPC", string(""));
        vm.skip(bytes(rpc).length == 0);
        vm.createSelectFork(rpc);
    }

    /// Launches through the factory's enabled config, with creator fees going to `feeRecipient`.
    function launch(address feeRecipient, uint16 taxBps) internal returns (address token, address curve) {
        uint256 config = enabledConfig();
        uint256 launchFee = FACTORY.launchFee();
        address launcher = makeAddr("launcher");
        vm.deal(launcher, 1 ether);
        vm.prank(launcher);
        (token, curve) = FACTORY.launchToken{value: launchFee}(params(feeRecipient, taxBps), config, address(0));
    }

    /// Names a launch to the burner, with the pool terms of the factory's enabled config.
    function bindTo(Burner burner, address token, address curve) internal {
        ILaunchFactory.LaunchConfig memory terms = FACTORY.getLaunchConfig(enabledConfig());
        burner.bind(IBurnableToken(token), ICurveBuy(curve), terms.poolFee, terms.tickSpacing);
    }

    /// Past the opening snipe tax, someone buys 1 ETH on the curve.
    function buyOnce(address curve) internal {
        vm.warp(vm.getBlockTimestamp() + 60);
        vm.roll(vm.getBlockNumber() + 60);
        address buyer = makeAddr("buyer");
        vm.deal(buyer, 1 ether);
        vm.prank(buyer);
        ICurveTrade(curve).buy{value: 1 ether}(1 ether, 0, buyer);
    }

    /// The keeper burns: everything the burner holds buys the token, and every token bought leaves the supply.
    function assertBurns(Burner burner, address keeper, IBurnableToken token) internal returns (uint256 burned) {
        uint256 supply = token.totalSupply();
        vm.prank(keeper);
        burned = burner.burn(1);
        assertGt(burned, 0, "the burn share bought tokens");
        assertEq(token.totalSupply(), supply - burned);
        assertEq(address(burner).balance, 0, "every wei went into the buy");
    }

    /// Launches with the splitter as fee recipient, buys 1 ETH past the snipe tax, and sweeps the curve's fees.
    function feesFromOneBuy(uint16 taxBps) internal returns (uint256) {
        address payable team = payable(makeAddr("team"));
        BountyPool pool = new BountyPool(makeAddr("judge"), team);
        FeeSplitter splitter =
            new FeeSplitter(ESCROW, payable(address(pool)), payable(makeAddr("burner")), team, 2_000, 3_000);
        (, address curve) = launch(address(splitter), taxBps);
        buyOnce(curve);
        splitter.sweepCurve(ILaunchCurve(curve));
        return ESCROW.balanceOf(address(splitter));
    }

    function params(address feeRecipient, uint16 taxBps) internal pure returns (ILaunchFactory.TokenParams memory) {
        return ILaunchFactory.TokenParams({
            name: "Kea Fork Test",
            symbol: "KEAFT",
            logo: "",
            description: "",
            socials: ILaunchFactory.Socials("", "", "", "", ""),
            creatorFeeRecipient: feeRecipient,
            creatorTaxBps: taxBps,
            buybackEnabled: false,
            expectedEconomics: bytes32(0),
            salt: keccak256(abi.encode("kea fork test", taxBps))
        });
    }

    function enabledConfig() internal view returns (uint256 id) {
        for (id = FACTORY.launchConfigCount(); id > 0; --id) {
            if (FACTORY.getLaunchConfig(id - 1).enabled) return id - 1;
        }
        revert("no enabled launch config");
    }
}
