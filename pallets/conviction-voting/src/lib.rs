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
    Delegations, MetadataOwner, PropIndex, ReferendumIndex, ReferendumInfo, ReferendumStatus,
    Tally, UnvoteScope,  ProposalType
};
pub use vote::{AccountVote, Vote, Voting};
pub use vote_threshold::{Approved, VoteThreshold};
pub use weights::WeightInfo;

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
    }

    /// The number of (public) proposals that have been made so far.
    #[pallet::storage]
    pub type PublicPropCount<T> = StorageValue<_, PropIndex, ValueQuery>;

    /// The public proposals. Unsorted. The second item is the proposal.
    #[pallet::storage]
    pub type PublicProps<T: Config> = StorageValue<
        _,
        BoundedVec<(PropIndex, BoundedCallOf<T>, T::AccountId), T::MaxProposals>,
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

    /// The next free referendum index, aka the number of referenda started so far.
    #[pallet::storage]
    pub type ReferendumCount<T> = StorageValue<_, ReferendumIndex, ValueQuery>;

    /// The lowest referendum index representing an unbaked referendum. Equal to
    /// `ReferendumCount` if there isn't a unbaked referendum.
    #[pallet::storage]
    pub type LowestUnbaked<T> = StorageValue<_, ReferendumIndex, ValueQuery>;

    /// Information concerning any given referendum.
    ///
    /// TWOX-NOTE: SAFE as indexes are not under an attacker’s control.
    #[pallet::storage]
    pub type ReferendumInfoOf<T: Config> = StorageMap<
        _,
        Twox64Concat,
        ReferendumIndex,
        ReferendumInfo<BlockNumberFor<T>, BoundedCallOf<T>, BalanceOf<T>>,
    >;


	 /// TWOX-NOTE: SAFE as indexes are not under an attacker’s control.
	#[pallet::storage]
	pub type ReferendumProposalType<T: Config> = StorageMap<
		_,
		Twox64Concat,
		ReferendumIndex,
		ProposalType<T::AccountId, BalanceOf<T>>,
	>;


    /// All votes for a particular voter. We store the balance for the number of votes that we
    /// have recorded. The second item is the total amount of delegations, that will be added.
    ///
    /// TWOX-NOTE: SAFE as `AccountId`s are crypto hashes anyway.
    #[pallet::storage]
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
        /// A user has successfully set a new value.
        SomethingStored {
            /// The new value set.
            something: u32,
            /// The account who set the new value.
            who: T::AccountId,
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
        /// The value retrieved was `None` as no value was previously set.
        NoneValue,
        /// There was an attempt to increment the value in storage over `u32::MAX`.
        StorageOverflow,
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
        /// An example dispatchable that takes a single u32 value as a parameter, writes the value
        /// to storage and emits an event.
        ///
        /// It checks that the _origin_ for this call is _Signed_ and returns a dispatch
        /// error if it isn't. Learn more about origins here: <https://docs.substrate.io/build/origins/>
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::do_something())]
        pub fn do_something(origin: OriginFor<T>, something: u32) -> DispatchResult {
            // Check that the extrinsic was signed and get the signer.
            let who = ensure_signed(origin)?;

            // Emit an event.
            Self::deposit_event(Event::SomethingStored { something, who });

            // Return a successful `DispatchResult`
            Ok(())
        }

        /// An example dispatchable that may throw a custom error.
        ///
        /// It checks that the caller is a signed origin and reads the current value from the
        /// `Something` storage item. If a current value exists, it is incremented by 1 and then
        /// written back to storage.
        ///
        /// ## Errors
        ///
        /// The function will return an error under the following conditions:
        ///
        /// - If no value has been set ([`Error::NoneValue`])
        /// - If incrementing the value in storage causes an arithmetic overflow
        ///   ([`Error::StorageOverflow`])
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::cause_error())]
        pub fn cause_error(origin: OriginFor<T>) -> DispatchResult {
            let _who = ensure_signed(origin)?;

           
			Ok(())
        }
    }
}
