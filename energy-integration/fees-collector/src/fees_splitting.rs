elrond_wasm::imports!();

use crate::{fees_accumulation::TokenAmountPair, week_timekeeping::Week};

pub type TokenAmountPairsVec<M> = ManagedVec<M, TokenAmountPair<M>>;
pub type PaymentsVec<M> = ManagedVec<M, EsdtTokenPayment<M>>;

#[elrond_wasm::module]
pub trait FeesSplittingModule:
    crate::config::ConfigModule
    + crate::week_timekeeping::WeekTimekeepingModule
    + crate::fees_accumulation::FeesAccumulationModule
    + crate::energy_query::EnergyQueryModule
{
    #[endpoint(claimRewards)]
    fn claim_rewards(&self, week: Week) {
        let current_week = self.get_current_week();
        require!(week <= current_week, "Invalid week number");

        let caller = self.blockchain().get_caller();
        let total_rewards = self.collect_and_get_rewards_for_week(week);
        let user_rewards = self.get_user_rewards_for_week(&caller, week, &total_rewards);
        if !user_rewards.is_empty() {
            self.send().direct_multi(&caller, &user_rewards);
        }

        self.add_energy_for_next_week(caller, current_week);
    }

    fn collect_and_get_rewards_for_week(&self, week: Week) -> TokenAmountPairsVec<Self::Api> {
        let total_rewards_mapper = self.total_rewards_for_week(week);
        if total_rewards_mapper.is_empty() {
            let total_rewards = self.collect_accumulated_fees_for_week(week);
            total_rewards_mapper.set(&total_rewards);

            total_rewards
        } else {
            total_rewards_mapper.get()
        }
    }

    fn get_user_rewards_for_week(
        &self,
        user: &ManagedAddress,
        week: Week,
        total_rewards: &TokenAmountPairsVec<Self::Api>,
    ) -> PaymentsVec<Self::Api> {
        let mut user_rewards = ManagedVec::new();
        let user_energy = self.user_energy_for_week(user, week).get();
        if user_energy == 0 {
            return user_rewards;
        }

        let total_energy = self.total_energy_for_week(week).get();
        for weekly_reward in total_rewards {
            let reward_amount = weekly_reward.amount * &user_energy / &total_energy;
            if reward_amount > 0 {
                user_rewards.push(EsdtTokenPayment::new(weekly_reward.token, 0, reward_amount));
            }
        }

        user_rewards
    }

    fn add_energy_for_next_week(&self, user: ManagedAddress, current_week: usize) {
        let next_week = current_week + 1;
        let user_energy_mapper = self.user_energy_for_week(&user, next_week);

        let previous_energy = user_energy_mapper.get();
        let user_energy_for_next_week = self.get_energy_non_zero(user);
        user_energy_mapper.set(&user_energy_for_next_week);

        self.total_energy_for_week(next_week).update(|total| {
            *total -= previous_energy;
            *total += user_energy_for_next_week
        });
    }

    #[storage_mapper("totalRewardsForWeek")]
    fn total_rewards_for_week(
        &self,
        week: Week,
    ) -> SingleValueMapper<ManagedVec<TokenAmountPair<Self::Api>>>;

    #[storage_mapper("totalUsersForWeek")]
    fn total_users_for_week(&self, week: Week) -> SingleValueMapper<usize>;

    #[storage_mapper("userEnergyForWeek")]
    fn user_energy_for_week(&self, user: &ManagedAddress, week: Week)
        -> SingleValueMapper<BigUint>;

    #[storage_mapper("totalEnergyForWeek")]
    fn total_energy_for_week(&self, week: Week) -> SingleValueMapper<BigUint>;
}
