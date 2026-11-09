use anchor_lang::prelude::*;

use crate::{error::ErrorCode, state::Escrow, ESCROW_SEED};

#[derive(Accounts)]
pub struct Update<'info> {
    // Only the maker can change the terms of their own offer.
    // The maker doesn't need to be writable: no lamports move, only the escrow's data changes.
    pub maker: Signer<'info>,
    #[account(
        mut,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, Escrow>,
}

impl<'info> Update<'info> {
    //Change what the maker wants in return and/or how long the offer stays open.
    //`None` means "leave this field as it is".
    pub fn update_escrow(&mut self, receive: Option<u64>, expiration: Option<i64>) -> Result<()> {
        require!(
            receive.is_some() || expiration.is_some(),
            ErrorCode::NothingToUpdate
        );

        if let Some(receive) = receive {
            require!(receive > 0, ErrorCode::InvalidAmount);
            self.escrow.receive = receive;
        }

        if let Some(expiration) = expiration {
            require!(
                expiration > Clock::get()?.unix_timestamp,
                ErrorCode::InvalidExpiration
            );
            self.escrow.expiration = expiration;
        }

        Ok(())
    }
}
