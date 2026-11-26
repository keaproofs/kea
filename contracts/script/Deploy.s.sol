// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Script, console} from "forge-std/Script.sol";
import {BountyPool} from "../src/BountyPool.sol";
import {Burner} from "../src/Burner.sol";
import {FeeSplitter, IFeeEscrow} from "../src/FeeSplitter.sol";

/// Deploys the bounty pool, the burner and the splitter, each before the one that needs it. JUDGE and TEAM are the
/// team's Safe, KEEPER is the server's keeper wallet, and whoever broadcasts becomes the burner's binder.
/// JUDGE=0x.. TEAM=0x.. KEEPER=0x.. forge script script/Deploy.s.sol --rpc-url <rpc> --broadcast <wallet options>
contract Deploy is Script {
    IFeeEscrow internal constant ESCROW = IFeeEscrow(0xd3AFEB2a57f70eF218Aa82451c51B2fb0416Ac9e);
    uint256 internal constant POOL_BPS = 2_000;
    uint256 internal constant BURN_BPS = 3_000;

    function run() external returns (BountyPool pool, Burner burner, FeeSplitter splitter) {
        address judge = vm.envAddress("JUDGE");
        address payable team = payable(vm.envAddress("TEAM"));
        address keeper = vm.envAddress("KEEPER");
        vm.startBroadcast();
        pool = new BountyPool(judge, team);
        burner = new Burner(keeper);
        splitter = new FeeSplitter(ESCROW, payable(address(pool)), payable(address(burner)), team, POOL_BPS, BURN_BPS);
        vm.stopBroadcast();
        console.log("POOL=%s", address(pool));
        console.log("BURNER=%s", address(burner));
        console.log("SPLITTER=%s", address(splitter));
    }
}
