elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use common_structs::{Epoch, Nonce, NonceAmountPair, Week};
use mergeable::Mergeable;
use simple_lock::locked_token::LockedTokenAttributes;

use crate::{lock_options::EPOCHS_PER_MONTH, token_merging::LockedAmountAttributesPair};

const MAX_PERCENTAGE: u16 = 10_000; // 100%
const MIN_EPOCHS_TO_REDUCE: Epoch = 1;
const EPOCHS_PER_WEEK: Epoch = 7;
static INVALID_PERCENTAGE_ERR_MSG: &[u8] = b"Invalid percentage value";

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct PenaltyPercentage {
    pub min: u16,
    pub max: u16,
}

pub mod fees_collector_proxy {
    elrond_wasm::imports!();

    #[elrond_wasm::proxy]
    pub trait FeesCollectorProxy {
        #[payable("*")]
        #[endpoint(depositSwapFees)]
        fn deposit_swap_fees(&self);
    }
}

#[elrond_wasm::module]
pub trait UnlockWithPenaltyModule:
    simple_lock::basic_lock_unlock::BasicLockUnlock
    + simple_lock::locked_token::LockedTokenModule
    + simple_lock::token_attributes::TokenAttributesModule
    + elrond_wasm_modules::default_issue_callbacks::DefaultIssueCallbacksModule
    + crate::energy::EnergyModule
    + crate::lock_options::LockOptionsModule
    + crate::events::EventsModule
    + elrond_wasm_modules::pause::PauseModule
    + utils::UtilsModule
{
    /// - min_penalty_percentage / max_penalty_percentage: The penalty for early unlock
    ///     of a token. A token locked for the max period, will have max_penalty_percentage penalty,
    ///     whereas one with 1 epoch left, will have min_penalty_percentage.
    ///     Penalty decreases linearly from max to min, based on the remaining locking period.
    ///     
    ///     Both are values between 0 and 10_000, where 10_000 is 100%.
    #[only_owner]
    #[endpoint(setPenaltyPercentage)]
    fn set_penalty_percentage(&self, min_penalty_percentage: u16, max_penalty_percentage: u16) {
        let is_min_valid = min_penalty_percentage > 0 && min_penalty_percentage <= MAX_PERCENTAGE;
        let is_max_valid = max_penalty_percentage > 0 && max_penalty_percentage <= MAX_PERCENTAGE;
        let correct_order = min_penalty_percentage <= max_penalty_percentage;
        require!(
            is_min_valid && is_max_valid && correct_order,
            INVALID_PERCENTAGE_ERR_MSG
        );

        self.penalty_percentage().set(&PenaltyPercentage {
            min: min_penalty_percentage,
            max: max_penalty_percentage,
        });
    }

    /// Sets the percentage of fees that are burned. The rest are sent to the fees collector.
    /// Value between 0 and 10_000. 0 is also accepted.
    #[only_owner]
    #[endpoint(setFeesBurnPercentage)]
    fn set_fees_burn_percentage(&self, percentage: u16) {
        require!(percentage <= MAX_PERCENTAGE, INVALID_PERCENTAGE_ERR_MSG);

        self.fees_burn_percentage().set(percentage);
    }

    #[only_owner]
    #[endpoint(setFeesCollectorAddress)]
    fn set_fees_collector_address(&self, sc_address: ManagedAddress) {
        self.require_sc_address(&sc_address);
        self.fees_collector_address().set(&sc_address);
    }

    /// Unlock a locked token instantly. This incures a penalty.
    /// The longer the remaining locking time, the bigger the penalty.
    #[payable("*")]
    #[endpoint(unlockEarly)]
    fn unlock_early(&self) -> EsdtTokenPayment {
        self.reduce_lock_period_common(None)
    }

    /// Reduce the locking period of a locked token. This incures a penalty.
    /// The longer the reduction, the bigger the penalty.
    /// epochs_to_reduce must be a multiple of 30 (i.e. 1 month)
    #[payable("*")]
    #[endpoint(reduceLockPeriod)]
    fn reduce_lock_period(&self, epochs_to_reduce: Epoch) -> EsdtTokenPayment {
        require!(
            epochs_to_reduce % EPOCHS_PER_YEAR == 0,
            "May only reduce by multiples of 12 months (360 epochs)"
        );

        self.reduce_lock_period_common(Some(epochs_to_reduce))
    }

    fn reduce_lock_period_common(&self, opt_epochs_to_reduce: Option<Epoch>) -> EsdtTokenPayment {
        self.require_not_paused();

        let locked_token_mapper = self.locked_token();
        let payment = self.call_value().single_esdt();
        locked_token_mapper.require_same_token(&payment.token_identifier);

        let attributes: LockedTokenAttributes<Self::Api> =
            locked_token_mapper.get_token_attributes(payment.token_nonce);

        let epochs_to_reduce =
            self.resolve_opt_epochs_to_reduce(opt_epochs_to_reduce, attributes.unlock_epoch);
        let penalty_amount = self.calculate_penalty_amount(&payment.amount, epochs_to_reduce);

        locked_token_mapper.nft_burn(payment.token_nonce, &(&payment.amount - &penalty_amount));

        let current_epoch = self.blockchain().get_block_epoch();
        let caller = self.blockchain().get_caller();

        let mut energy = self.get_updated_energy_entry_for_user(&caller);
        energy.deplete_after_early_unlock(&payment.amount, attributes.unlock_epoch, current_epoch);

        let mut unlocked_tokens = self.unlock_tokens_unchecked(payment.clone(), &attributes);
        let unlocked_token_id = unlocked_tokens.token_identifier.clone().unwrap_esdt();
        let new_unlock_epoch = attributes.unlock_epoch - epochs_to_reduce;

        if new_unlock_epoch == current_epoch {
            self.send()
            .esdt_local_mint(&unlocked_token_id, 0, &unlocked_tokens.amount);
        }

        if penalty_amount > 0 {
            unlocked_tokens.amount -= &penalty_amount;
            require!(
                unlocked_tokens.amount > 0,
                "No tokens remaining after penalty is applied"
            );

            self.burn_penalty(locked_token_mapper.get_token_id(), payment.token_nonce, &penalty_amount);
        }

        let output_payment = self.lock_and_send(&caller, unlocked_tokens, new_unlock_epoch);

        energy.add_after_token_lock(&output_payment.amount, new_unlock_epoch, current_epoch);
        self.set_energy_entry(&caller, energy);

        self.to_esdt_payment(output_payment)
    }

    fn resolve_opt_epochs_to_reduce(
        &self,
        opt_epochs_to_reduce: Option<Epoch>,
        original_unlock_epoch: Epoch,
    ) -> Epoch {
        let current_epoch = self.blockchain().get_block_epoch();
        require!(
            original_unlock_epoch > current_epoch,
            "Token can be unlocked already"
        );

        let lock_epochs_remaining = original_unlock_epoch - current_epoch;
        match opt_epochs_to_reduce {
            Some(epochs_to_reduce) => {
                require!(
                    epochs_to_reduce >= MIN_EPOCHS_TO_REDUCE
                        && epochs_to_reduce <= lock_epochs_remaining,
                    "Invalid epochs to reduce"
                );

                epochs_to_reduce
            }
            None => lock_epochs_remaining,
        }
    }

    /// Calculates the penalty that would be incurred if token_amount tokens
    /// were to have their locking period reduce by epochs_to_reduce.
    ///
    /// Linear decrease as epochs_to_reduce decreases
    /// starting from max penalty_percentage, all the way down to min
    #[view(getPenaltyAmount)]
    fn calculate_penalty_amount(&self, token_amount: &BigUint, epochs_to_reduce: Epoch) -> BigUint {
        let penalty_percentage = self.penalty_percentage().get();
        let min_penalty = penalty_percentage.min as u64;
        let max_penalty = penalty_percentage.max as u64;
        let max_lock_option = self.max_lock_option().get();

        let penalty_percentage =
            min_penalty + (max_penalty - min_penalty) * epochs_to_reduce / max_lock_option;

        token_amount * penalty_percentage / MAX_PERCENTAGE as u64
    }

    fn burn_penalty(&self, token_id: TokenIdentifier, token_nonce: Nonce, fees_amount: &BigUint) {
        let fees_burn_percentage = self.fees_burn_percentage().get();
        let burn_amount = fees_amount * fees_burn_percentage as u64 / MAX_PERCENTAGE as u64;
        let remaining_amount = fees_amount - &burn_amount;

        if burn_amount > 0 {
            self.send().esdt_local_burn(&token_id, 0, &burn_amount);
        }
        if remaining_amount > 0 {
            if self.fees_from_penalty_unlocking().is_empty() {
                // First fee deposit of the week
                self.fees_from_penalty_unlocking()
                    .set(NonceAmountPair::new(token_nonce, remaining_amount));
            } else {
                self.merge_fees_from_penalty(token_nonce, &remaining_amount)
            }
        }

        // Send fees to FeeCollector SC
        let current_epoch = self.blockchain().get_block_epoch();
        let last_week_fee_sent_to_collector = self.last_week_fee_sent_to_collector().get();
        if current_epoch >= last_week_fee_sent_to_collector + EPOCHS_PER_WEEK {
            self.send_fees_to_collector();
        }
    }

    /// Merges new fees with existing fees and saves in storage
    fn merge_fees_from_penalty(&self, token_nonce: Nonce, new_fee_amount: &BigUint) {
        let locked_token_mapper = self.locked_token();
        let existing_nonce_amount_pair = self.fees_from_penalty_unlocking().get();
        let existing_token_attributes: LockedTokenAttributes<Self::Api> =
        locked_token_mapper.get_token_attributes(existing_nonce_amount_pair.nonce);
        let mut output_pair = LockedAmountAttributesPair {
            token_amount: existing_nonce_amount_pair.amount.clone(),
            attributes: existing_token_attributes,
        };

        locked_token_mapper.nft_burn(
            existing_nonce_amount_pair.nonce,
            &existing_nonce_amount_pair.amount,
        );

        let new_token_attributes: LockedTokenAttributes<Self::Api> =
        locked_token_mapper.get_token_attributes(token_nonce);
        let new_pair = LockedAmountAttributesPair {
            token_amount: new_fee_amount.clone(),
            attributes: new_token_attributes,
        };

        locked_token_mapper.nft_burn(token_nonce, &new_fee_amount);


        output_pair.merge_with(new_pair);

        self.fees_from_penalty_unlocking().set(NonceAmountPair::new(
            output_pair.attributes.original_token_nonce,
            output_pair.token_amount,
        ));
    }


    #[payable("*")]
    #[endpoint(sendFeesToCollector)]
    fn send_fees_to_collector(&self) {
        let sc_address = self.fees_collector_address().get();
        let locked_token_id = self.locked_token().get_token_id();
        let nonce_amount_pair = self.fees_from_penalty_unlocking().get();
        let locked_token_mapper = self.locked_token();

        locked_token_mapper.nft_burn(nonce_amount_pair.nonce, &nonce_amount_pair.amount);
        self.fees_from_penalty_unlocking().clear();

        self.fees_collector_proxy_builder(sc_address)
            .deposit_swap_fees()
            .add_esdt_token_transfer(
                locked_token_id,
                nonce_amount_pair.nonce,
                nonce_amount_pair.amount,
            )
            .execute_on_dest_context_ignore_result();
    }

    #[proxy]
    fn fees_collector_proxy_builder(
        &self,
        sc_address: ManagedAddress,
    ) -> fees_collector_proxy::Proxy<Self::Api>;

    #[view(getPenaltyPercentage)]
    #[storage_mapper("penaltyPercentage")]
    fn penalty_percentage(&self) -> SingleValueMapper<PenaltyPercentage>;

    #[view(getFeesBurnPercentage)]
    #[storage_mapper("feesBurnPercentage")]
    fn fees_burn_percentage(&self) -> SingleValueMapper<u16>;

    #[view(getFeesCollectorAddress)]
    #[storage_mapper("feesCollectorAddress")]
    fn fees_collector_address(&self) -> SingleValueMapper<ManagedAddress>;

    #[view(getFeesFromPenaltyUnlocking)]
    #[storage_mapper("feesFromPenaltyUnlocking")]
    fn fees_from_penalty_unlocking(&self) -> SingleValueMapper<NonceAmountPair<Self::Api>>;

    #[view(getLastWeekFeeSentToCollector)]
    #[storage_mapper("lastWeekFeeSentToCollector")]
    fn last_week_fee_sent_to_collector(&self) -> SingleValueMapper<Week>;

}
