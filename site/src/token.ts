/** $KEA's contract address on Robinhood Chain. Empty until the launch; the CA bar and the live chip read it. */
export const CA = "0xDA53a3D36F269397c9733f19b152172Ab8161839";

/** The contracts $KEA's creator fee runs through, deployed 27 Nov 2026 (verified on Sourcify). */
export const SPLITTER = "0x58E11c14C3c2C0f2152b76F79c8C609B8273a2C0";
export const POOL = "0x90b570e9d42861fE3FD6ce19Fe6464Bb875D3319";
/** Spends its 30% of the fee on $KEA and burns it; /burn follows its Burned events. */
export const BURNER = "0x910b892f6A730B9FE3BC68b953E5494faaAa1d5a";
/** The team's Safe, paid 50% of the fee. */
export const TEAM = "0xeB435a99fCDEd29DF806F9f505A0669d1403cA4B";
/** The account that runs the hourly burn, as the burner answers keeper(). */
export const KEEPER = "0x6b79990c20c4F5C913D4660cFFD26da6C8991b05";
export const EXPLORER = "https://robin.etherscan.io";
/** Robinhood Chain's public RPC. It answers browsers too, so /burn's last section reads the chain from the visitor's own
 * browser. */
export const RPC = "https://rpc.mainnet.chain.robinhood.com";
/** The open-source repository's files, for links to the code behind the page. */
export const REPO = "https://github.com/keaproofs/kea/blob/main";

/** The launch's creator wallet, and where its whole dev bag went at launch (tx DEV_BAG_TX). */
export const DEV = "0xf61815d056797A7A85f1dA3B9A060F4e2a658B0F";
export const DEAD = "0x000000000000000000000000000000000000dEaD";
export const DEV_BAG_TX = "0x2c030b870067de27af0958b3ef18cc108c347960a3a2fabc899663b15265859a";
/** That transfer's own amount in wei, from its Transfer log. Anyone can send more to DEAD, so the dead address's
 * balance is not this number. */
export const DEV_BAG = 22573888759599720735396788n;
