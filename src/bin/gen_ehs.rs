extern crate rust_poker;
// extern crate rayon;
extern crate bytepack;
extern crate crossbeam;

use bytepack::LEPacker;
use std::fs::OpenOptions;
use std::io;
use std::io::Write; // <--- ring flush() into scope
use std::time::Instant;

use hand_indexer::HandIndexer;
use rust_poker::constants::{RANK_TO_CHAR, SUIT_TO_CHAR};
use rust_poker::equity_calculator::approx_equity;
use rust_poker::hand_range::{Combo, HandRange};

const N_THREADS: u64 = 8;

fn main() {
    let cards_per_round: [usize; 4] = [2, 5, 6, 7];

    // create preflop indexer
    let indexers = [
        HandIndexer::init(1, [2].to_vec()),
        HandIndexer::init(2, [2, 3].to_vec()),
        HandIndexer::init(2, [2, 4].to_vec()),
        HandIndexer::init(2, [2, 5].to_vec()),
    ];

    // let mut file = File::create("ehs.dat").unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("ehs.dat")
        .unwrap();

    for i in 0..4 {
        let start_time = Instant::now();
        // number of isomorphic hands in this street
        let round = if i == 0 { 0 } else { 1 };
        let batch_size = indexers[i].size(round);
        println!("{} combinations in round {}", batch_size, i);
        // num hands per thread
        let size_per_thread = batch_size / N_THREADS;
        // equity table
        let mut equity_table = vec![0f64; batch_size as usize];
        // current round 0->preflop, 3->river
        crossbeam::scope(|scope| {
            for (j, slice) in equity_table
                .chunks_mut(size_per_thread as usize)
                .enumerate()
            {
                scope.spawn(move |_| {
                    let mut board_mask: u64;
                    let mut combo: Combo;
                    let mut hand_ranges: Vec<HandRange>;
                    let mut cards: Vec<u8> = vec![0; cards_per_round[i]];
                    for k in 0..slice.len() {
                        // update percent every 1000 hands on thread 0
                        if (j == 0) && (k & 0xfff == 0) {
                            print!("{:.3}% \r", (100 * k) as f64 / size_per_thread as f64);
                            io::stdout().flush().unwrap();
                        }

                        indexers[i].get_hand(
                            round,
                            ((j as u64) * size_per_thread) + (k as u64),
                            cards.as_mut_slice(),
                        );
                        combo = Combo(cards[0], cards[1], 100);

                        // create board
                        board_mask = 0;
                        let mut board_str = String::new();
                        for n in 2..cards_per_round[i as usize] {
                            board_mask |= 1u64 << cards[n];
                            board_str.push(RANK_TO_CHAR[(cards[n] >> 2) as usize]);
                            board_str.push(SUIT_TO_CHAR[(cards[n] & 3) as usize]);
                        }

                        hand_ranges = HandRange::from_strings(
                            [combo.to_string(), "random".to_string()].to_vec(),
                        );

                        // run sim
                        if i == 0 {
                            slice[k] =
                                approx_equity(&mut hand_ranges, board_mask, 1, 0.001).unwrap()[0];
                        } else {
                            // small sample count and more cores
                            slice[k] =
                                approx_equity(&mut hand_ranges, board_mask, 2, 0.01).unwrap()[0];
                        }
                    }
                });
            }
        })
        .unwrap();

        // write to file
        file.pack_all(&equity_table[..]).unwrap();

        let duration = start_time.elapsed().as_millis();
        println!(
            "round {} done. took {}ms ({:.2} iterations / ms)",
            i,
            duration,
            batch_size as f64 / duration as f64
        );
    }
}
