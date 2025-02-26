//! # Conviction Voting

// We make sure this pallet uses `no_std` for compiling to Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

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
pub mod types;
pub mod weights;
pub use types::{Conviction, Proposal, ProposalId, ProposalType, Tally};
pub use weights::*;

use codec::{Decode, Encode};
use frame_support::pallet_prelude::*;
use frame_support::{
    dispatch::DispatchResult,
    pallet_prelude::*,
	traits::LockIdentifier,
    traits::{Currency, LockableCurrency, ReservableCurrency, WithdrawReasons, ExistenceRequirement},
};

use frame_system::pallet_prelude::*;
use sp_runtime::traits::{AccountIdConversion, Saturating, StaticLookup};

use frame_system::pallet_prelude::{BlockNumberFor, OriginFor};

type BalanceOf<T> =
    <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
type NegativeImbalanceOf<T> = <<T as Config>::Currency as Currency<
    <T as frame_system::Config>::AccountId,
>>::NegativeImbalance;

pub(crate) const CONVICTION_ID: LockIdentifier = *b"convicti";



// All pallet logic is defined in its own module and must be annotated by the `pallet` attribute.
#[frame_support::pallet(dev_mode)]
pub mod pallet {
    // Import various useful types required by all FRAME pallets.
    use super::*;

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
        /// A type representing the weights required by the dispatchables of this pallet.
        ///
        type Currency: ReservableCurrency<Self::AccountId> + LockableCurrency<Self::AccountId>;
        type VotingPeriod: Get<BlockNumberFor<Self>>;
        type ProposalMinStake: Get<BalanceOf<Self>>;
        type ValidatorMinStake: Get<BalanceOf<Self>>;
        type MaxValidators: Get<u32>;
        type ProposalHandler: ProposalHandler<Self>;
        type WeightInfo: WeightInfo;
    }

    /// A storage item for this pallet.
    ///
    /// In this template, we are declaring a storage item called `Something` that stores a single
    /// `u32` value. Learn more about runtime storage here: <https://docs.substrate.io/build/runtime-storage/>
    #[pallet::storage]
    #[pallet::getter(fn proposals)]
    pub type Proposals<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        ProposalId,
        Proposal<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
    >;

    #[pallet::storage]
    #[pallet::getter(fn next_proposal_id)]
    pub type NextProposalId<T> = StorageValue<_, ProposalId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_validators)]
    pub type ActiveValidators<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn validator_stakes)]
    pub type ValidatorStakes<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, BalanceOf<T>>;
    /// Events that functions in this pallet can emit.
    ///
    /// Events are a simple means of indicating to the outside world (such as dApps, chain explorers
    /// or other users) that some notable update in the runtime has occurred. In a FRAME pallet, the
    /// documentation for each event field and its parameters is added to a node's metadata so it
    /// can be used by external interfaces or tools.
    ///
    ///	The `generate_deposit` macro generates a function on `Pallet` called `deposit_event` which
    /// will convert the event type of your pallet into `RuntimeEvent` (declared in the pallet's
    /// [`Config`] trait) and deposit it using [`frame_system::Pallet::deposit_event`].
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ProposalCreated(ProposalId, ProposalType<T::AccountId, BalanceOf<T>>),
        Voted(T::AccountId, ProposalId, Conviction, BalanceOf<T>),
        ProposalApproved(ProposalId),
        ProposalRejected(ProposalId),
        ValidatorSelected(T::AccountId, BalanceOf<T>),
        ValidatorRemoved(T::AccountId, BalanceOf<T>),
        FundsGranted(T::AccountId, BalanceOf<T>),
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
        InsufficientStake,
        InvalidProposal,
        AlreadyVoted,
        VotingClosed,
        InvalidConviction,
        ValidatorSlotFull,
        NotAValidator,
        Unauthorized,
    }

    // Proposal handler trait
    pub trait ProposalHandler<T: Config> {
        fn handle_approved_proposal(
            proposal_type: ProposalType<T::AccountId, BalanceOf<T>>,
            stake: BalanceOf<T>,
            proposer: T::AccountId,
        ) -> DispatchResult;
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
        #[pallet::weight(0)]
        pub fn create_proposal(
            origin: OriginFor<T>,
            proposal_type: ProposalType<T::AccountId, BalanceOf<T>>,
            stake: BalanceOf<T>,
        ) -> DispatchResult {
            let proposer = ensure_signed(origin)?;

            // Validate stake based on proposal type
            match &proposal_type {
                ProposalType::ValidatorApplication { .. } => {
                    ensure!(
                        stake >= T::ValidatorMinStake::get(),
                        Error::<T>::InsufficientStake
                    );
                }
                _ => {
                    ensure!(
                        stake >= T::ProposalMinStake::get(),
                        Error::<T>::InsufficientStake
                    );
                }
            }

            T::Currency::reserve(&proposer, stake)?;

            let id = NextProposalId::<T>::get();
            let now = frame_system::Pallet::<T>::block_number();
            let end_block = now + T::VotingPeriod::get();

            let proposal = Proposal {
                proposer: proposer.clone(),
                proposal_type: proposal_type.clone(),
                stake,
                start_block: now,
                end_block,
                tally: Tally::default(),
                voters: Vec::new(),
            };

            Proposals::<T>::insert(id, proposal);
            NextProposalId::<T>::put(id + 1);

            Self::deposit_event(Event::ProposalCreated(id, proposal_type));
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(0)]
        pub fn vote(
            origin: OriginFor<T>,
            proposal_id: ProposalId,
            conviction: Conviction,
            in_favor: bool,
        ) -> DispatchResult {
            let voter = ensure_signed(origin)?;
            let mut proposal =
                Proposals::<T>::get(proposal_id).ok_or(Error::<T>::InvalidProposal)?;

            ensure!(
                frame_system::Pallet::<T>::block_number() < proposal.end_block,
                Error::<T>::VotingClosed
            );

            ensure!(!proposal.voters.contains(&voter), Error::<T>::AlreadyVoted);

            let voting_power = conviction.voting_power(T::Currency::free_balance(&voter));
            let lock_periods = conviction.lock_periods();
            let unlock_block = proposal
                .end_block
                .saturating_add(T::VotingPeriod::get().saturating_mul(lock_periods.into()));

            T::Currency::extend_lock(
				CONVICTION_ID,
                &voter,
                voting_power,
                WithdrawReasons::except(WithdrawReasons::RESERVE),
            );

            if in_favor {
                proposal.tally.in_favor += voting_power;
            } else {
                proposal.tally.against += voting_power;
            }

            proposal.tally.total_power += voting_power;
            proposal.voters.push(voter.clone());

            Proposals::<T>::insert(proposal_id, proposal);
            Self::deposit_event(Event::Voted(voter, proposal_id, conviction, voting_power));
            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(0)]
        pub fn finalize_proposal(origin: OriginFor<T>, proposal_id: ProposalId) -> DispatchResult {
            ensure_root(origin)?;
            let proposal =
                Proposals::<T>::get(proposal_id).ok_or(Error::<T>::InvalidProposal)?;

            ensure!(
                frame_system::Pallet::<T>::block_number() >= proposal.end_block,
                Error::<T>::VotingClosed
            );

            let threshold = proposal.tally.total_power / 2u32.into();
            let approved = proposal.tally.in_favor > threshold;

            if approved {
                T::ProposalHandler::handle_approved_proposal(
                    proposal.proposal_type.clone(),
                    proposal.stake,
                    proposal.proposer.clone(),
                )?;
                Self::deposit_event(Event::ProposalApproved(proposal_id));
            } else {
                T::Currency::unreserve(&proposal.proposer, proposal.stake);
                Self::deposit_event(Event::ProposalRejected(proposal_id));
            }

            Proposals::<T>::remove(proposal_id);
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(0)]
        pub fn remove_validator(origin: OriginFor<T>, validator: T::AccountId) -> DispatchResult {
            ensure_root(origin)?;
            Self::_remove_validator(&validator)?;
            Ok(())
        }
    }


	impl<T: Config> Pallet<T> {
		fn _remove_validator(validator: &T::AccountId) -> DispatchResult {
			ActiveValidators::<T>::mutate(|validators| {
				if let Some(pos) = validators.iter().position(|v| v == validator) {
					validators.remove(pos);
					
					if let Some(stake) = ValidatorStakes::<T>::take(validator) {
						T::Currency::remove_lock(CONVICTION_ID, validator);
						T::Currency::unreserve(validator, stake);
						Self::deposit_event(Event::ValidatorRemoved(
							validator.clone(),
							stake,
						));
					}
					Ok(())
				} else {
					Err(Error::<T>::NotAValidator.into())
				}
			})
		}
	
		pub fn is_validator(account: &T::AccountId) -> bool {
			ActiveValidators::<T>::get().contains(account)
		}
	
		fn add_validator(
			validator: T::AccountId,
			stake: BalanceOf<T>,
		) -> DispatchResult {
			ActiveValidators::<T>::mutate(|validators| {
				ensure!(
					(validators.len() as u32) < T::MaxValidators::get(),
					Error::<T>::ValidatorSlotFull
				);
				
				// Lock validator stake
				T::Currency::extend_lock(
					CONVICTION_ID,
					&validator,
					stake,
					WithdrawReasons::except(WithdrawReasons::RESERVE),
				);
				
				ValidatorStakes::<T>::insert(&validator, stake);
				validators.push(validator.clone());
				
				Self::deposit_event(Event::ValidatorSelected(
					validator,
					stake,
				));
				Ok(())
			})
		}
	}
	
	// Example runtime implementation
	impl<T: Config> ProposalHandler<T> for Pallet<T> {
		fn handle_approved_proposal(
			proposal_type: ProposalType<T::AccountId, BalanceOf<T>>,
			stake: BalanceOf<T>,
			proposer: T::AccountId,
		) -> DispatchResult {
			match proposal_type {
				ProposalType::Grant { recipient, amount } => {
					// Handle grant payout logic
					// // (Assuming treasury account exists)
					// let treasury = T::Treasury::get();
					// T::Currency::transfer(
					// 	&treasury,
					// 	&recipient,
					// 	amount,
					// 	ExistenceRequirement::AllowDeath,
					// )?;
					Self::deposit_event(Event::FundsGranted(recipient, amount));
				},
				ProposalType::ValidatorApplication { validator, .. } => {
					// Ensure proposer is the validator applicant
					ensure!(proposer == validator, Error::<T>::Unauthorized);
					
					// Add validator with locked stake
					Self::add_validator(validator, stake)?;
				},
			}
			Ok(())
		}
	}
	
	
}

