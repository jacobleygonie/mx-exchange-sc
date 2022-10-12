////////////////////////////////////////////////////
////////////////// AUTO-GENERATED //////////////////
////////////////////////////////////////////////////

#![no_std]

elrond_wasm_node::wasm_endpoints! {
    farm_staking
    (
        callBack
        addAdmin
        addSCAddressToWhitelist
        addToPauseWhitelist
        calculateRewardsForGivenPosition
        claimRewards
        claimRewardsWithNewValue
        compoundRewards
        end_produce_rewards
        getAccumulatedRewards
        getAnnualPercentageRewards
        getDivisionSafetyConstant
        getFarmTokenId
        getFarmTokenSupply
        getFarmingTokenId
        getLastRewardBlockNonce
        getMinUnbondEpochs
        getPerBlockRewardAmount
        getPermissions
        getRewardCapacity
        getRewardPerShare
        getRewardTokenId
        getState
        isSCAddressWhitelisted
        mergeFarmTokens
        pause
        registerFarmToken
        removeAdmin
        removeFromPauseWhitelist
        removeSCAddressFromWhitelist
        resume
        setMaxApr
        setMinUnbondEpochs
        setPerBlockRewardAmount
        stakeFarm
        stakeFarmThroughProxy
        startProduceRewards
        topUpRewards
        unbondFarm
        unstakeFarm
        unstakeFarmThroughProxy
    )
}
