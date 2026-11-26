// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Script, console} from "forge-std/Script.sol";
import {Burner, IBurnableToken, ICurveBuy} from "../src/Burner.sol";
import {ILaunchFactory} from "../test/Fork.t.sol";

/// Launch day on a local copy of the chain (anvil): a token whose fees go to an already deployed splitter, with a 2%
/// creator tax, named to the burner. Trading and the keeper come after, from the command line.
/// REHEARSAL=1 SPLITTER=0x.. BURNER=0x.. forge script script/Rehearse.s.sol --rpc-url http://127.0.0.1:8545 --broadcast
///   --unlocked --sender <the account that deployed the burner>
contract Rehearse is Script {
    ILaunchFactory internal constant FACTORY = ILaunchFactory(0x7eD598BcEf8bd9Edd8C97A195C6d13f40801EC7e);

    function run() external {
        require(vm.envOr("REHEARSAL", false), "rehearsal only: set REHEARSAL=1 and point at a local fork");
        address splitter = vm.envAddress("SPLITTER");
        Burner burner = Burner(payable(vm.envAddress("BURNER")));
        uint256 config = enabledConfig();
        ILaunchFactory.LaunchConfig memory terms = FACTORY.getLaunchConfig(config);
        uint256 launchFee = FACTORY.launchFee();

        vm.startBroadcast();
        (address token, address curve) = FACTORY.launchToken{value: launchFee}(params(splitter), config, address(0));
        burner.bind(IBurnableToken(token), ICurveBuy(curve), terms.poolFee, terms.tickSpacing);
        vm.stopBroadcast();
        console.log("TOKEN=%s", token);
        console.log("CURVE=%s", curve);
    }

    function params(address feeRecipient) internal view returns (ILaunchFactory.TokenParams memory) {
        return ILaunchFactory.TokenParams({
            name: "Kea Rehearsal",
            symbol: "KEAR",
            logo: "",
            description: "",
            socials: ILaunchFactory.Socials("", "", "", "", ""),
            creatorFeeRecipient: feeRecipient,
            creatorTaxBps: 200,
            buybackEnabled: false,
            expectedEconomics: bytes32(0),
            salt: keccak256(abi.encode("kea rehearsal", block.timestamp))
        });
    }

    function enabledConfig() internal view returns (uint256 id) {
        for (id = FACTORY.launchConfigCount(); id > 0; --id) {
            if (FACTORY.getLaunchConfig(id - 1).enabled) return id - 1;
        }
        revert("no enabled launch config");
    }
}
