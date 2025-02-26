use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{
    traits::{Bounded, CheckedDiv, CheckedMul, Zero},
    RuntimeDebug,
    Perbill,
};

pub type ProposalId = u32;

#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo)]
pub enum ProposalType<AccountId, Balance> {
    Grant {
        recipient: AccountId,
        amount: Balance,
    },
    ValidatorApplication {
        validator: AccountId,
        commission: Perbill,
    },
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo)]
pub struct Proposal<AccountId, Balance, BlockNumber> {
    pub proposer: AccountId,
    pub proposal_type: ProposalType<AccountId, Balance>,
    pub stake: Balance,
    pub start_block: BlockNumber,
    pub end_block: BlockNumber,
    pub tally: Tally<Balance>,
    pub voters: Vec<AccountId>,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, Default, TypeInfo)]
pub struct Tally<Balance> {
    pub total_power: Balance,
    pub in_favor: Balance,
    pub against: Balance,
}

// Conviction voting implementation
#[derive(Encode, Decode, Copy, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub enum Conviction {
    None,
    Locked1x,
    Locked2x,
    Locked3x,
    Locked4x,
    Locked5x,
    Locked6x,
}

impl Default for Conviction {
	fn default() -> Self {
		Conviction::None
	}
}

impl From<Conviction> for u8 {
	fn from(c: Conviction) -> u8 {
		match c {
			Conviction::None => 0,
			Conviction::Locked1x => 1,
			Conviction::Locked2x => 2,
			Conviction::Locked3x => 3,
			Conviction::Locked4x => 4,
			Conviction::Locked5x => 5,
			Conviction::Locked6x => 6,
		}
	}
}


impl TryFrom<u8> for Conviction {
	type Error = ();
	fn try_from(i: u8) -> Result<Conviction, ()> {
		Ok(match i {
			0 => Conviction::None,
			1 => Conviction::Locked1x,
			2 => Conviction::Locked2x,
			3 => Conviction::Locked3x,
			4 => Conviction::Locked4x,
			5 => Conviction::Locked5x,
			6 => Conviction::Locked6x,
			_ => return Err(()),
		})
	}
}


impl Conviction {
    pub fn lock_periods(self) -> u32 {
        match self {
            Conviction::None => 0,
            Conviction::Locked1x => 1,
            Conviction::Locked2x => 2,
            Conviction::Locked3x => 4,
            Conviction::Locked4x => 8,
            Conviction::Locked5x => 16,
            Conviction::Locked6x => 32,
        }
    }

    pub fn voting_power<Balance: From<u8> + CheckedMul + CheckedDiv + Bounded + Zero>(
        self,
        balance: Balance,
    ) -> Balance {
        match self {
            Conviction::None => balance.checked_div(&10u8.into()).unwrap_or_else(Zero::zero),
            x => balance
                .checked_mul(&u8::from(x).into())
                .unwrap_or_else(Bounded::max_value),
        }
    }
}


impl Bounded for Conviction {
	fn min_value() -> Self {
		Conviction::None
	}
	fn max_value() -> Self {
		Conviction::Locked6x
	}
}



