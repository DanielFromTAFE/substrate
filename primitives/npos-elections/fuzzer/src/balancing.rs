// This file is part of Substrate.

// Copyright (C) 2020 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Fuzzing fro the balance_solution algorithm
//!
//! It ensures that any solution which gets equalized will lead into a better or equally scored
//! one.

mod common;

use common::to_range;
use honggfuzz::fuzz;
use sp_npos_elections::{
	assignment_ratio_to_staked_normalized, build_support_map, to_without_backing, seq_phragmen,
	ElectionResult, VoteWeight, evaluate_support, is_score_better,
};
use sp_std::collections::btree_map::BTreeMap;
use sp_runtime::Perbill;
use rand::{self, Rng, SeedableRng, RngCore};

type AccountId = u64;

fn generate_random_npos_result(
	voter_count: u64,
	target_count: u64,
	to_elect: usize,
	mut rng: impl RngCore,
) -> (
	ElectionResult<AccountId, Perbill>,
	Vec<AccountId>,
	Vec<(AccountId, VoteWeight, Vec<AccountId>)>,
	BTreeMap<AccountId, VoteWeight>,
) {
	let prefix = 100_000;
	// Note, it is important that stakes are always bigger than ed.
	let base_stake: u64 = 1_000_000_000;
	let ed: u64 = base_stake;

	let mut candidates = Vec::with_capacity(target_count as usize);
	let mut stake_of: BTreeMap<AccountId, VoteWeight> = BTreeMap::new();

	(1..=target_count).for_each(|acc| {
		candidates.push(acc);
		let stake_var = rng.gen_range(ed, 100 * ed);
		stake_of.insert(acc, base_stake + stake_var);
	});

	let mut voters = Vec::with_capacity(voter_count as usize);
	(prefix ..= (prefix + voter_count)).for_each(|acc| {
		let edge_per_this_voter = rng.gen_range(1, candidates.len());
		// all possible targets
		let mut all_targets = candidates.clone();
		// we remove and pop into `targets` `edge_per_this_voter` times.
		let targets = (0..edge_per_this_voter).map(|_| {
			let upper = all_targets.len() - 1;
			let idx = rng.gen_range(0, upper);
			all_targets.remove(idx)
		})
		.collect::<Vec<AccountId>>();

		let stake_var = rng.gen_range(ed, 100 * ed) ;
		let stake = base_stake + stake_var;
		stake_of.insert(acc, stake);
		voters.push((acc, stake, targets));
	});

	(
		seq_phragmen::<AccountId, sp_runtime::Perbill>(
			to_elect,
			candidates.clone(),
			voters.clone(),
			None,
		).unwrap(),
		candidates,
		voters,
		stake_of,
	)
}

fn main() {
	loop {
		fuzz!(|data: (usize, usize, usize, usize, u64)| {
			let (
				mut target_count,
				mut voter_count,
				mut iterations,
				mut to_elect,
				seed,
			) = data;
			let rng = rand::rngs::SmallRng::seed_from_u64(seed);
			target_count = to_range(target_count, 50, 2000);
			voter_count = to_range(voter_count, 50, 1000);
			iterations = to_range(iterations, 1, 50);
			to_elect = to_range(to_elect, 25, target_count);

			println!(
				"++ [voter_count: {} / target_count:{} / to_elect:{} / iterations:{}]",
				voter_count, target_count, to_elect, iterations,
			);
			let (
				unbalanced,
				candidates,
				voters,
				stake_of_tree,
			) = generate_random_npos_result(
				voter_count as u64,
				target_count as u64,
				to_elect,
				rng,
			);

			let stake_of = |who: &AccountId| -> VoteWeight {
				*stake_of_tree.get(who).unwrap()
			};

			let unbalanced_score = {
				let staked = assignment_ratio_to_staked_normalized(unbalanced.assignments.clone(), &stake_of).unwrap();
				let winners = to_without_backing(unbalanced.winners);
				let support = build_support_map(winners.as_ref(), staked.as_ref()).0;

				let score = evaluate_support(&support);
				if score[0] == 0 {
					// such cases cannot be improved by reduce.
					return;
				}
				score
			};

			let balanced = seq_phragmen::<AccountId, sp_runtime::Perbill>(
				to_elect,
				candidates,
				voters,
				Some((iterations, 0)),
			).unwrap();

			let balanced_score = {
				let staked = assignment_ratio_to_staked_normalized(balanced.assignments.clone(), &stake_of).unwrap();
				let winners = to_without_backing(balanced.winners);
				let support = build_support_map(winners.as_ref(), staked.as_ref()).0;
				evaluate_support(&support)
			};

			let enhance = is_score_better(balanced_score, unbalanced_score, Perbill::zero());

			println!(
				"iter = {} // {:?} -> {:?} [{}]",
				iterations,
				unbalanced_score,
				balanced_score,
				enhance,
			);

			// The only guarantee of balancing is such that the first and third element of the score
			// cannot decrease.
			assert!(
				balanced_score[0] >= unbalanced_score[0] &&
				balanced_score[1] == unbalanced_score[1] &&
				balanced_score[2] <= unbalanced_score[2]
			);
		});
	}
}
