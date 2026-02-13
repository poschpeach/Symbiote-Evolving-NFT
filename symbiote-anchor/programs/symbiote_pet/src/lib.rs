use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, MintTo, SetAuthority, Token, TokenAccount};
use mpl_token_metadata::instructions::{
    CreateMasterEditionV3, CreateMasterEditionV3InstructionArgs, CreateMetadataAccountV3,
    CreateMetadataAccountV3InstructionArgs, UpdateMetadataAccountV2,
    UpdateMetadataAccountV2InstructionArgs,
};
use mpl_token_metadata::types::DataV2;
use spl_token::instruction::AuthorityType;

declare_id!("Fg6PaFpoGXkYsidMpWxTWqkZq5Q8x8M9KXQvS6kR7d5k");

const MAX_PERSONALITY_LEN: usize = 64;
const URI_BASE: &str = "http://localhost:3000/metadata";
const NAME_PREFIX: &str = "Symbiote Pet #";
const SYMBOL: &str = "SYMB";

#[program]
pub mod symbiote_pet {
    use super::*;

    pub fn mint_symbiote(ctx: Context<MintSymbiote>, owner: Pubkey) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.owner.key(),
            owner,
            SymbioteError::OwnerPubkeyMismatch
        );

        assert_metadata_pda(ctx.accounts.metadata.key(), ctx.accounts.mint.key(), false)?;
        assert_metadata_pda(
            ctx.accounts.master_edition.key(),
            ctx.accounts.mint.key(),
            true,
        )?;

        let bump = ctx.bumps.symbiote_state;
        let state = &mut ctx.accounts.symbiote_state;
        state.bump = bump;
        state.owner = owner;
        state.evolution_authority = ctx.accounts.payer.key();
        state.mint = ctx.accounts.mint.key();
        state.level = 1;
        state.xp = 0;
        state.personality = "Neutral".to_string();
        state.uri = build_uri(state.mint, state.level, state.xp, &state.personality);

        let signer_seeds: &[&[u8]] = &[
            b"symbiote_state",
            ctx.accounts.mint.key().as_ref(),
            &[ctx.bumps.symbiote_state],
        ];

        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.owner_ata.to_account_info(),
                    authority: ctx.accounts.symbiote_state.to_account_info(),
                },
                &[signer_seeds],
            ),
            1,
        )?;

        let metadata_data = DataV2 {
            name: format!("{}{}", NAME_PREFIX, short_mint(&ctx.accounts.mint.key())),
            symbol: SYMBOL.to_string(),
            uri: state.uri.clone(),
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        };

        let metadata_ix = CreateMetadataAccountV3 {
            metadata: ctx.accounts.metadata.key(),
            mint: ctx.accounts.mint.key(),
            mint_authority: ctx.accounts.symbiote_state.key(),
            payer: ctx.accounts.payer.key(),
            update_authority: (ctx.accounts.symbiote_state.key(), true),
            system_program: ctx.accounts.system_program.key(),
            rent: Some(ctx.accounts.rent.key()),
        }
        .instruction(CreateMetadataAccountV3InstructionArgs {
            data: metadata_data,
            is_mutable: true,
            collection_details: None,
        });

        invoke_signed(
            &metadata_ix,
            &[
                ctx.accounts.metadata.to_account_info(),
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.symbiote_state.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.rent.to_account_info(),
                ctx.accounts.token_metadata_program.to_account_info(),
            ],
            &[signer_seeds],
        )?;

        let edition_ix = CreateMasterEditionV3 {
            edition: ctx.accounts.master_edition.key(),
            mint: ctx.accounts.mint.key(),
            update_authority: ctx.accounts.symbiote_state.key(),
            mint_authority: ctx.accounts.symbiote_state.key(),
            payer: ctx.accounts.payer.key(),
            metadata: ctx.accounts.metadata.key(),
            token_program: ctx.accounts.token_program.key(),
            system_program: ctx.accounts.system_program.key(),
            rent: Some(ctx.accounts.rent.key()),
        }
        .instruction(CreateMasterEditionV3InstructionArgs {
            max_supply: Some(0),
        });

        invoke_signed(
            &edition_ix,
            &[
                ctx.accounts.master_edition.to_account_info(),
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.symbiote_state.to_account_info(),
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.metadata.to_account_info(),
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.rent.to_account_info(),
                ctx.accounts.token_metadata_program.to_account_info(),
            ],
            &[signer_seeds],
        )?;

        token::set_authority(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                SetAuthority {
                    account_or_mint: ctx.accounts.mint.to_account_info(),
                    current_authority: ctx.accounts.symbiote_state.to_account_info(),
                },
                &[signer_seeds],
            ),
            AuthorityType::MintTokens,
            None,
        )?;

        Ok(())
    }

    pub fn evolve_symbiote(
        ctx: Context<EvolveSymbiote>,
        nft_account: Pubkey,
        new_stats: Stats,
    ) -> Result<()> {
        assert_metadata_pda(ctx.accounts.metadata.key(), nft_account, false)?;

        let state = &mut ctx.accounts.symbiote_state;
        require_keys_eq!(state.mint, nft_account, SymbioteError::NftMintMismatch);
        require_keys_eq!(
            ctx.accounts.authority.key(),
            state.evolution_authority,
            SymbioteError::UnauthorizedEvolutionAuthority
        );
        require!(
            new_stats.personality_string.len() <= MAX_PERSONALITY_LEN,
            SymbioteError::PersonalityTooLong
        );

        state.level = new_stats.level;
        state.xp = new_stats.xp;
        state.personality = new_stats.personality_string.clone();
        state.uri = build_uri(state.mint, state.level, state.xp, &state.personality);

        let signer_seeds: &[&[u8]] = &[b"symbiote_state", state.mint.as_ref(), &[state.bump]];

        let metadata_data = DataV2 {
            name: format!("{}{}", NAME_PREFIX, short_mint(&state.mint)),
            symbol: SYMBOL.to_string(),
            uri: state.uri.clone(),
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        };

        let update_ix = UpdateMetadataAccountV2 {
            metadata: ctx.accounts.metadata.key(),
            update_authority: ctx.accounts.symbiote_state.key(),
        }
        .instruction(UpdateMetadataAccountV2InstructionArgs {
            data: Some(metadata_data),
            new_update_authority: None,
            primary_sale_happened: None,
            is_mutable: Some(true),
        });

        invoke_signed(
            &update_ix,
            &[
                ctx.accounts.metadata.to_account_info(),
                ctx.accounts.symbiote_state.to_account_info(),
                ctx.accounts.token_metadata_program.to_account_info(),
            ],
            &[signer_seeds],
        )?;

        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct Stats {
    pub level: u16,
    pub xp: u64,
    pub personality_string: String,
}

#[derive(Accounts)]
pub struct MintSymbiote<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: Validated against argument.
    pub owner: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        mint::decimals = 0,
        mint::authority = symbiote_state,
        mint::freeze_authority = symbiote_state
    )]
    pub mint: Account<'info, Mint>,
    #[account(
        init,
        payer = payer,
        seeds = [b"symbiote_state", mint.key().as_ref()],
        bump,
        space = SymbioteState::space()
    )]
    pub symbiote_state: Account<'info, SymbioteState>,
    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = owner
    )]
    pub owner_ata: Account<'info, TokenAccount>,
    /// CHECK: PDA verified in handler.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,
    /// CHECK: PDA verified in handler.
    #[account(mut)]
    pub master_edition: UncheckedAccount<'info>,
    /// CHECK: Program address check.
    #[account(address = mpl_token_metadata::ID)]
    pub token_metadata_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(nft_account: Pubkey)]
pub struct EvolveSymbiote<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"symbiote_state", nft_account.as_ref()],
        bump = symbiote_state.bump,
        constraint = symbiote_state.mint == nft_account @ SymbioteError::NftMintMismatch
    )]
    pub symbiote_state: Account<'info, SymbioteState>,
    /// CHECK: PDA must match mint metadata PDA.
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,
    /// CHECK: Program address check.
    #[account(address = mpl_token_metadata::ID)]
    pub token_metadata_program: UncheckedAccount<'info>,
}

#[account]
pub struct SymbioteState {
    pub bump: u8,
    pub owner: Pubkey,
    pub evolution_authority: Pubkey,
    pub mint: Pubkey,
    pub level: u16,
    pub xp: u64,
    pub personality: String,
    pub uri: String,
}

impl SymbioteState {
    pub fn space() -> usize {
        8 + 1 + 32 + 32 + 32 + 2 + 8 + 4 + MAX_PERSONALITY_LEN + 4 + 200
    }
}

#[error_code]
pub enum SymbioteError {
    #[msg("Provided owner pubkey does not match owner account.")]
    OwnerPubkeyMismatch,
    #[msg("NFT account/mint mismatch.")]
    NftMintMismatch,
    #[msg("Only the configured evolution authority can evolve the Symbiote.")]
    UnauthorizedEvolutionAuthority,
    #[msg("Personality string exceeds max length.")]
    PersonalityTooLong,
    #[msg("Metadata account does not match the expected PDA.")]
    InvalidMetadataPda,
}

fn short_mint(mint: &Pubkey) -> String {
    mint.to_string().chars().take(6).collect()
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else if c.is_ascii_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                '_'
            }
        })
        .collect()
}

fn build_uri(mint: Pubkey, level: u16, xp: u64, personality: &str) -> String {
    format!(
        "{}/{}/state.json?level={}&xp={}&personality={}",
        URI_BASE,
        mint,
        level,
        xp,
        sanitize(personality)
    )
}

fn assert_metadata_pda(metadata: Pubkey, mint: Pubkey, edition: bool) -> Result<()> {
    let seeds = if edition {
        vec![
            b"metadata".as_ref(),
            mpl_token_metadata::ID.as_ref(),
            mint.as_ref(),
            b"edition".as_ref(),
        ]
    } else {
        vec![
            b"metadata".as_ref(),
            mpl_token_metadata::ID.as_ref(),
            mint.as_ref(),
        ]
    };
    let (expected, _) = Pubkey::find_program_address(&seeds, &mpl_token_metadata::ID);
    require_keys_eq!(expected, metadata, SymbioteError::InvalidMetadataPda);
    Ok(())
}
