use crate::types::{ProposalType};
use crate::{AccountVote, Conviction, Vote, Voting};
use crate::{mock::*, Error, Event};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::BoundedVec;
use sp_runtime::traits::ConstU32;

#[test]
fn it_works_for_default_value() {
    new_test_ext().execute_with(|| {
        // Go past genesis block so events get deposited
        System::set_block_number(1);

        let proposal = vec![1, 2, 3];

        let bounded_proposal: BoundedVec<u8, ConstU32<32>> = proposal.try_into().unwrap();
        let proposal_type = ProposalType::Grant {
            recipient: 2, // AccountId of the recipient
            amount: 100,  // Amount to grant
        };

        let deposit = 100;

        assert_ok!(ConvictionVoting::propose(
            RuntimeOrigin::signed(1),
            proposal_type,
            bounded_proposal,
            deposit
        ));
    });
}

#[test]
fn vote_with_sufficient_funds_works() {
    new_test_ext().execute_with(|| {


        let proposal = vec![1, 2, 3];

        let bounded_proposal: BoundedVec<u8, ConstU32<32>> = proposal.try_into().unwrap();
        assert_ok!(ConvictionVoting::propose(
            RuntimeOrigin::signed(1),
            ProposalType::Grant { recipient: 2, amount: 50 },
            bounded_proposal,
            10,
        ));

        // Vote on the proposal
        assert_ok!(ConvictionVoting::vote(
            RuntimeOrigin::signed(1),
            0,
            AccountVote::Standard {
                vote: Vote {
                    aye: true,
                    conviction: Conviction::None,
                },
                balance: 10,
            },
        ));

        // Check that the vote was recorded
        let voting = ConvictionVoting::voting_of(&1);
        println!("{:?}", voting);

        assert_ok!(ConvictionVoting::vote(
            RuntimeOrigin::signed(3),
            0,
            AccountVote::Standard {
                vote: Vote {
                    aye: true,
                    conviction: Conviction::Locked4x,
                },
                balance: 10,
            },
        ));

        let voting= ConvictionVoting::voting_of(&3);
        println!("{:?}", voting);
        // assert_eq!(voting, Voting::Direct { votes: BoundedVec([(0, AccountVote::Standard { vote: Vote { aye: true, conviction: Conviction::None }, balance: 10 })], 1000), delegations: Delegations { votes: 0, capital: 0 }, prior: PriorLock(0, 0) });
        let referendum_info = ConvictionVoting::referendum_info_of(&0);
        println!("{:?}", referendum_info);
        

        // For second voters Conviction::Locked2x
        // Some(ReferendumInfo::Ongoing(ReferendumStatus { end: 1001, proposal: BoundedVec([1, 2, 3], 32), threshold: VoteThreshold::SuperMajorityApprove, delay: 100, tally: Tally { ayes: 21, nays: 0, turnout: 20 } }))

        // With both voters Conviction:None
        // Some(ReferendumInfo::Ongoing(ReferendumStatus { end: 1001, proposal: BoundedVec([1, 2, 3], 32), threshold: VoteThreshold::SuperMajorityApprove, delay: 100, tally: Tally { ayes: 2, nays: 0, turnout: 20 } }))
        


        assert_ok!(ConvictionVoting::vote(
            RuntimeOrigin::signed(4),
            0,
            AccountVote::Standard {
                vote: Vote {
                    aye: false,
                    conviction: Conviction::Locked4x,
                },
                balance: 10,
            },
        ));

        let voting= ConvictionVoting::voting_of(&4);
        println!("{:?}", voting);
        // assert_eq!(voting, Voting::Direct { votes: BoundedVec([(0, AccountVote::Standard { vote: Vote { aye: true, conviction: Conviction::None }, balance: 10 })], 1000), delegations: Delegations { votes: 0, capital: 0 }, prior: PriorLock(0, 0) });
        let referendum_info = ConvictionVoting::referendum_info_of(&0);
        println!("{:?}", referendum_info);

        // Some(ReferendumInfo::Ongoing(ReferendumStatus { end: 1001, proposal: BoundedVec([1, 2, 3], 32), threshold: VoteThreshold::SuperMajorityApprove, delay: 100, tally: Tally { ayes: 41, nays: 40, turnout: 30 } }))
    });
}
