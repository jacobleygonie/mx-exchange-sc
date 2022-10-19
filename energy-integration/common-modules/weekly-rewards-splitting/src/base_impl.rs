elrond_wasm::imports!();

use common_types::TokenAmountPairsVec;
use week_timekeeping::Week;

use crate::{events, ClaimProgress};

pub trait AllBaseWeeklyRewardsSplittingImplTraits = crate::WeeklyRewardsSplittingModule
    + energy_query::EnergyQueryModule
    + week_timekeeping::WeekTimekeepingModule
    + events::WeeklyRewardsSplittingEventsModule;

pub trait WeeklyRewardsSplittingTraitsModule {
    type WeeklyRewardsSplittingMod: AllBaseWeeklyRewardsSplittingImplTraits;

    fn collect_and_get_rewards_for_week_base(
        &self,
        module: &Self::WeeklyRewardsSplittingMod,
        week: Week,
    ) -> TokenAmountPairsVec<<Self::WeeklyRewardsSplittingMod as ContractBase>::Api> {
        let total_rewards_mapper = module.total_rewards_for_week(week);
        if total_rewards_mapper.is_empty() {
            let total_rewards = Self::collect_rewards_for_week(&self, module, week);
            total_rewards_mapper.set(&total_rewards);

            total_rewards
        } else {
            total_rewards_mapper.get()
        }
    }

    fn collect_rewards_for_week(
        &self,
        module: &Self::WeeklyRewardsSplittingMod,
        week: Week,
    ) -> TokenAmountPairsVec<<Self::WeeklyRewardsSplittingMod as ContractBase>::Api>;

    fn get_current_claim_progress(
        &self,
        module: &Self::WeeklyRewardsSplittingMod,
        user: &ManagedAddress<<Self::WeeklyRewardsSplittingMod as ContractBase>::Api>,
    ) -> SingleValueMapper<
        <Self::WeeklyRewardsSplittingMod as ContractBase>::Api,
        ClaimProgress<<Self::WeeklyRewardsSplittingMod as ContractBase>::Api>,
    > {
        module.current_claim_progress(user)
    }
}
