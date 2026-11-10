use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("The escrow has expired")]
    EscrowExpired,
    #[msg("The expiration must be in the future")]
    InvalidExpiration,
    #[msg("The amount must be greater than zero")]
    InvalidAmount,
    #[msg("Nothing to update: pass a new receive amount, a new expiration, or both")]
    NothingToUpdate,
}
