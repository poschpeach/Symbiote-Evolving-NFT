import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair } from "@solana/web3.js";
import { getAssociatedTokenAddressSync } from "@solana/spl-token";
import { strict as assert } from "assert";

describe("symbiote_pet", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.SymbiotePet as Program;
  const tokenMetadataProgram = new PublicKey("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

  it("mints and evolves symbiote nft state", async () => {
    const owner = provider.wallet.publicKey;
    const mint = Keypair.generate();
    const [symbioteState] = PublicKey.findProgramAddressSync(
      [Buffer.from("symbiote_state"), mint.publicKey.toBuffer()],
      program.programId
    );
    const [metadata] = PublicKey.findProgramAddressSync(
      [Buffer.from("metadata"), tokenMetadataProgram.toBuffer(), mint.publicKey.toBuffer()],
      tokenMetadataProgram
    );
    const [masterEdition] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        tokenMetadataProgram.toBuffer(),
        mint.publicKey.toBuffer(),
        Buffer.from("edition"),
      ],
      tokenMetadataProgram
    );
    const ownerAta = getAssociatedTokenAddressSync(mint.publicKey, owner);

    await program.methods
      .mintSymbiote(owner)
      .accounts({
        payer: provider.wallet.publicKey,
        owner,
        mint: mint.publicKey,
        symbioteState,
        ownerAta,
        metadata,
        masterEdition,
        tokenMetadataProgram,
      })
      .signers([mint])
      .rpc();

    const initial = await program.account.symbioteState.fetch(symbioteState);
    assert.equal(initial.level, 1);
    assert.equal(initial.xp.toNumber(), 0);
    assert.equal(initial.personality, "Neutral");

    await program.methods
      .evolveSymbiote(mint.publicKey, {
        level: 3,
        xp: new anchor.BN(2500),
        personalityString: "Degen",
      })
      .accounts({
        authority: provider.wallet.publicKey,
        symbioteState,
        metadata,
        tokenMetadataProgram,
      })
      .rpc();

    const evolved = await program.account.symbioteState.fetch(symbioteState);
    assert.equal(evolved.level, 3);
    assert.equal(evolved.xp.toNumber(), 2500);
    assert.equal(evolved.personality, "Degen");
  });
});
