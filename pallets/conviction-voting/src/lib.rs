//! # Conviction Voting

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{vec, vec::Vec};
use codec::{Decode, Encode};
use frame_support::{
    ensure,
    traits::{
        defensive_prelude::*,
        schedule::{v3::Named as ScheduleNamed, DispatchTime},
        Bounded, Currency, EnsureOrigin, Get, LockIdentifier, LockableCurrency, OnUnbalanced,
        QueryPreimage, ReservableCurrency, StorePreimage, WithdrawReasons,
    },
    weights::Weight,
};
use frame_system::pallet_prelude::{BlockNumberFor, OriginFor};
use sp_runtime::SaturatedConversion;
use sp_runtime::{
    traits::{BadOrigin, Bounded as ArithBounded, One, Saturating, StaticLookup, Zero},
    ArithmeticError, DispatchError, DispatchResult,
};

mod conviction;
mod types;
mod vote;
mod vote_threshold;
pub mod weights;
pub use conviction::Conviction;
pub use pallet::*;
pub use types::{
    Delegations, MetadataOwner, PropIndex, ProposalType, ReferendumIndex, ReferendumInfo,
    ReferendumStatus, Tally, UnvoteScope,
};
pub use vote::{AccountVote, Vote, Voting};
pub use vote_threshold::{Approved, VoteThreshold};
pub use weights::WeightInfo;
use frame_support::BoundedVec;
use frame_support::traits::ConstU32;

// Re-export pallet items so that they can be accessed from the crate namespace.
pub use pallet::*;

// FRAME pallets require their own "mock runtimes" to be able to run unit tests. This module
// contains a mock runtime specific for testing this pallet's functionality.
#[cfg(test)]
mod mock;

// This module contains the unit tests for this pallet.
// Learn about pallet unit testing here: https://docs.substrate.io/test/unit-testing/
#[cfg(test)]
mod tests;

// Every callable function or "dispatchable" a pallet exposes must have weight values that correctly
// estimate a dispatchable's execution time. The benchmarking module is used to calculate weights
// for each dispatchable and generates this pallet's weight.rs file. Learn more about benchmarking here: https://docs.substrate.io/test/benchmark/
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub(crate) const CONVICTION_VOTING: LockIdentifier = *b"convicti";

type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
type NegativeImbalanceOf<T> = <<T as Config>::Currency as Currency<
    <T as frame_system::Config>::AccountId,
>>::NegativeImbalance;
pub type CallOf<T> = <T as frame_system::Config>::RuntimeCall;
pub type BoundedCallOf<T> = Bounded<CallOf<T>, <T as frame_system::Config>::Hashing>;
type AccountIdLookupOf<T> = <<T as frame_system::Config>::Lookup as StaticLookup>::Source;

#[frame_support::pallet(dev_mode)]
pub mod pallet {
    // Import various useful types required by all FRAME pallets.
    use super::{DispatchResult, *};
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;

    // The `Pallet` struct serves as a placeholder to implement traits, methods and dispatchables
    // (`Call`s) in this pallet.
    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// The pallet's configuration trait.
    ///
    /// All our types and constants a pallet depends on must be declared here.
    /// These types are defined generically and made concrete when the pallet is declared in the
    /// `runtime/src/lib.rs` file of your chain.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching runtime event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Currency type for this pallet.
        type Currency: ReservableCurrency<Self::AccountId>
            + LockableCurrency<Self::AccountId, Moment = BlockNumberFor<Self>>;
        /// A type representing the weights required by the dispatchables of this pallet.
        type WeightInfo: WeightInfo;

        #[pallet::constant]
        type MaxVotes: Get<u32>;

        /// The maximum number of public proposals that can exist at any time.
        #[pallet::constant]
        type MaxProposals: Get<u32>;

        /// The maximum number of deposits a public proposal may have at any time.
        #[pallet::constant]
        type MaxDeposits: Get<u32>;

        #[pallet::constant]
        type MinimumDeposit: Get<BalanceOf<Self>>;
    }

    /// The number of (public) proposals that have been made so far.
    #[pallet::storage]
    pub type PublicPropCount<T> = StorageValue<_, PropIndex, ValueQuery>;

    /// The public proposals. Unsorted. The second item is the proposal.
    #[pallet::storage]
    pub type PublicProps<T: Config> = StorageValue<
        _,
        BoundedVec<(PropIndex, BoundedVec<u8, ConstU32<32>>, T::AccountId), T::MaxProposals>,
        ValueQuery,
    >;

    /// Those who have locked a deposit.
    ///
    /// TWOX-NOTE: Safe, as increasing integer keys are safe.
    #[pallet::storage]
    pub type DepositOf<T: Config> = StorageMap<
        _,
        Twox64Concat,
        PropIndex,
        (BoundedVec<T::AccountId, T::MaxDeposits>, BalanceOf<T>),
    >;

    #[pallet::storage]
    pub type ReferendumInfoOf<T: Config> = StorageMap<
        _,
        Twox64Concat,
        PropIndex,
        ReferendumInfo<BlockNumberFor<T>, BoundedVec<u8, ConstU32<32>>, BalanceOf<T>>,
    >;

    /// TWOX-NOTE: SAFE as indexes are not under an attacker’s control.
    #[pallet::storage]
    pub type ProposalTypeStore<T: Config> =
        StorageMap<_, Twox64Concat, PropIndex, ProposalType<T::AccountId, BalanceOf<T>>>;

    /// All votes for a particular voter. We store the balance for the number of votes that we
    /// have recorded. The second item is the total amount of delegations, that will be added.
    ///
    /// TWOX-NOTE: SAFE as `AccountId`s are crypto hashes anyway.
    #[pallet::storage]
    #[pallet::getter(fn voting_of)]
    pub type VotingOf<T: Config> = StorageMap<
        _,
        Twox64Concat,
        T::AccountId,
        Voting<BalanceOf<T>, T::AccountId, BlockNumberFor<T>, T::MaxVotes>,
        ValueQuery,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A motion has been proposed by a public account.
        Proposed {
            proposal_index: PropIndex,
            deposit: BalanceOf<T>,
        },

        /// An account has voted in a referendum
        Voted {
            voter: T::AccountId,
            ref_index: ReferendumIndex,
            vote: AccountVote<BalanceOf<T>>,
        },
    }

    /// Errors that can be returned by this pallet.
    ///
    /// Errors tell users that something went wrong so it's important that their naming is
    /// informative. Similar to events, error documentation is added to a node's metadata so it's
    /// equally important that they have helpful documentation associated with them.
    ///
    /// This type of runtime error can be up to 4 bytes in size should you want to return additional
    /// information.
    #[pallet::error]
    pub enum Error<T> {
        /// Value too low
        ValueLow,
        /// Maximum number of items reached.
        TooMany,
        /// Proposal still blacklisted
        ProposalBlacklisted,

        InsufficientFunds,

        MaxVotesReached,

        AlreadyDelegating,

        ReferendumInvalid,
    }

    /// The pallet's dispatchable functions ([`Call`]s).
    ///
    /// Dispatchable functions allows users to interact with the pallet and invoke state changes.
    /// These functions materialize as "extrinsics", which are often compared to transactions.
    /// They must always return a `DispatchResult` and be annotated with a weight and call index.
    ///
    /// The [`call_index`] macro is used to explicitly
    /// define an index for calls in the [`Call`] enum. This is useful for pallets that may
    /// introduce new dispatchables over time. If the order of a dispatchable changes, its index
    /// will also change which will break backwards compatibility.
    ///
    /// The [`weight`] macro is used to assign a weight to each call.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Propose a sensitive action to be taken.
        ///
        /// The dispatch origin of this call must be _Signed_ and the sender must
        /// have funds to cover the deposit.
        ///
        /// - `proposal_hash`: The hash of the proposal preimage.
        /// - `value`: The amount of deposit (must be at least `MinimumDeposit`).
        ///
        /// Emits `Proposed`.
        #[pallet::call_index(0)]
        #[pallet::weight(0)]
        pub fn propose(
            origin: OriginFor<T>,
            proposal_type: ProposalType<T::AccountId, BalanceOf<T>>,
            proposal: BoundedVec<u8, ConstU32<32>>,
            #[pallet::compact] value: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(value >= T::MinimumDeposit::get(), Error::<T>::ValueLow);

            let index = PublicPropCount::<T>::get();
            let real_prop_count = PublicProps::<T>::decode_len().unwrap_or(0) as u32;
            let max_proposals = T::MaxProposals::get();
            ensure!(real_prop_count < max_proposals, Error::<T>::TooMany);

            // if let Some((until, _)) = Blacklist::<T>::get(proposal_hash) {
            // 	ensure!(
            // 		frame_system::Pallet::<T>::block_number() >= until,
            // 		Error::<T>::ProposalBlacklisted,
            // 	);
            // }

            T::Currency::reserve(&who, value)?;

            let depositors = BoundedVec::<_, T::MaxDeposits>::truncate_from(vec![who.clone()]);
            DepositOf::<T>::insert(index, (depositors, value));
            ProposalTypeStore::<T>::insert(index, proposal_type);
            PublicPropCount::<T>::put(index + 1);

            PublicProps::<T>::try_append((index, proposal.clone(), who))
                .map_err(|_| Error::<T>::TooMany)?;

            // Change it
            let now = frame_system::Pallet::<T>::block_number();
            let voting_period = Self::u64_to_block_saturated(1000);
            let delay = Self::u64_to_block_saturated(100);
            let end = now.saturating_add(voting_period);
            let threshold = VoteThreshold::SuperMajorityApprove;
            let status = ReferendumStatus {
                end,
                proposal,
                threshold,
                delay,
                tally: Default::default(),
            };
            let item = ReferendumInfo::Ongoing(status);
            ReferendumInfoOf::<T>::insert(index, item);

            Self::deposit_event(Event::<T>::Proposed {
                proposal_index: index,
                deposit: value,
            });
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(0)]
        pub fn vote(
            origin: OriginFor<T>,
            #[pallet::compact] ref_index: PropIndex,
            vote: AccountVote<BalanceOf<T>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::try_vote(&who, ref_index, vote)
        }
    }
}

impl<T: Config> Pallet<T> {
    fn try_vote(
        who: &T::AccountId,
        ref_index: PropIndex,
        vote: AccountVote<BalanceOf<T>>,
    ) -> DispatchResult {
        let mut status = Self::referendum_status(ref_index)?;
        ensure!(
            vote.balance() <= T::Currency::free_balance(who),
            Error::<T>::InsufficientFunds
        );
        VotingOf::<T>::try_mutate(who, |voting| -> DispatchResult {
            if let Voting::Direct {
                ref mut votes,
                delegations,
                ..
            } = voting
            {
                match votes.binary_search_by_key(&ref_index, |i| i.0) {
                    Ok(i) => {
                        // Shouldn't be possible to fail, but we handle it gracefully.
                        status
                            .tally
                            .remove(votes[i].1)
                            .ok_or(ArithmeticError::Underflow)?;
                        if let Some(approve) = votes[i].1.as_standard() {
                            status.tally.reduce(approve, *delegations);
                        }
                        votes[i].1 = vote;
                    }
                    Err(i) => {
                        votes
                            .try_insert(i, (ref_index, vote))
                            .map_err(|_| Error::<T>::MaxVotesReached)?;
                    }
                }
                Self::deposit_event(Event::<T>::Voted {
                    voter: who.clone(),
                    ref_index,
                    vote,
                });
                // Shouldn't be possible to fail, but we handle it gracefully.
                status.tally.add(vote).ok_or(ArithmeticError::Overflow)?;
                if let Some(approve) = vote.as_standard() {
                    status.tally.increase(approve, *delegations);
                }
                Ok(())
            } else {
                Err(Error::<T>::AlreadyDelegating.into())
            }
        })?;
        // Extend the lock to `balance` (rather than setting it) since we don't know what other
        // votes are in place.
        T::Currency::extend_lock(
            CONVICTION_VOTING,
            who,
            vote.balance(),
            WithdrawReasons::except(WithdrawReasons::RESERVE),
        );
        ReferendumInfoOf::<T>::insert(ref_index, ReferendumInfo::Ongoing(status));
        Ok(())
    }

    fn referendum_status(
        ref_index: ReferendumIndex,
    ) -> Result<ReferendumStatus<BlockNumberFor<T>, BoundedVec<u8, ConstU32<32>>, BalanceOf<T>>, DispatchError>
    {
        let info = ReferendumInfoOf::<T>::get(ref_index).ok_or(Error::<T>::ReferendumInvalid)?;
        Self::ensure_ongoing(info)
    }

    fn ensure_ongoing(
        r: ReferendumInfo<BlockNumberFor<T>, BoundedVec<u8, ConstU32<32>>,BalanceOf<T>>,
    ) -> Result<ReferendumStatus<BlockNumberFor<T>, BoundedVec<u8, ConstU32<32>>, BalanceOf<T>>, DispatchError>
    {
        match r {
            ReferendumInfo::Ongoing(s) => Ok(s),
            _ => Err(Error::<T>::ReferendumInvalid.into()),
        }
    }

    pub fn u64_to_balance_saturated(input: u64) -> BalanceOf<T> {
        input.saturated_into::<BalanceOf<T>>()
    }

    pub fn u64_to_block_saturated(input: u64) -> BlockNumberFor<T> {
        input.saturated_into::<BlockNumberFor<T>>()
    }
}
