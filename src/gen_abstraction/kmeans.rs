use crossbeam::atomic::AtomicCell;

use std::sync::Arc;
use std::io::Write;
use std::io;
use std::time::{ Instant };

use rand::distributions::{WeightedIndex, Uniform};

use rand::Rng;
use rayon::prelude::*;
use std::cmp::Ordering;

use crate::{Histogram};

static EPSILON: f32 = 0.005;

pub struct Kmeans {
    centers: Vec<Histogram>
}

impl Kmeans {
    /**
     * Trys n times to initialize the centers for k-means
     * randomly chooses centers and return max distance one
     *
     * n_restarts: number of restarts,
     * n_centers: k in k-means,
     * center: k means to return,
     * dataset: reference to dataset,
     * rng: seeded rng
     */
    pub fn init_random<R: Rng>(
            n_restarts: usize, n_centers: usize, rng: &mut R,
            dist_func: &'static (dyn Fn(&Histogram, &Histogram) -> f32 + Sync),
            dataset: &Vec<Histogram>) -> Kmeans {

        let start = Instant::now();

        println!("Initializing Kmeans with {} random restarts", n_restarts);

        // create n centers to choose from
        let mut center_c: Vec<Vec<&Histogram>> = Vec::with_capacity(n_restarts);
        let n_data = dataset.len();
        // for picking first center
        let uniform_dist: Uniform<usize> = Uniform::from(0..n_data);
        // init centers randomly
        for i in 0..n_restarts {
            // random init
            center_c.push(Vec::with_capacity(n_centers));
            for _ in 0..n_centers {
             center_c[i].push(&dataset[rng.sample(uniform_dist)]);
            }
        }

        // calculate total dists of each restart
        let iteration = AtomicCell::new(0usize);
        let mut cluster_dists: Vec<f32> = vec![0f32; n_restarts];
        cluster_dists.par_iter_mut().enumerate().for_each(|(r, cd)| {

            let cur_iter = iteration.fetch_add(1);
            print!("Restart: {}/{}\r", cur_iter, n_restarts);
            io::stdout().flush().unwrap();

            let mut sum = 0f32;
            let mut count = 0usize;
            let mut distances = vec![0f32; n_centers];
            for i in 0..n_centers {
                for j in 0..n_centers {
                    if j == i {
                        continue;
                    }
                    let dist = dist_func(&center_c[r][i], &center_c[r][j]);
                    distances[i] += dist;
                    count += 1;
                }
                sum += distances[i];
            }
            *cd = sum / count as f32;
        });

        // get max index
        // use index of maximum (most spread out) clusters
        let max_cluster: usize = cluster_dists
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap();

        println!("Done.  Took {}ms", start.elapsed().as_millis());

        // clone center to return
        Kmeans {
            centers: center_c[max_cluster].iter().map(|x| (*x).clone()).collect()
        }
    }

    /**
     * Fit data to clusters
     * clusters: a mutable reference which contains the predictions
     * returns number of clusters that have changed (used for training)
     */
    pub fn predict(&self,
            dataset: &Vec<Histogram>,
            clusters: &mut Vec<usize>,
            dist_func: &'static (dyn Fn(&Histogram, &Histogram) -> f32 + Sync)) -> usize {

        if clusters.len() != dataset.len() {
            panic!("Cluster and dataset does not match");
        }

        // number of means
        let n_centers = self.centers.len();

        // number of clusters that have changed
        let changed = Arc::new(AtomicCell::new(0usize));

        clusters.par_iter_mut().enumerate().for_each(|(i, cluster)| {
            let changed = Arc::clone(&changed);
            let curr_cluster = *cluster;
            let mut variance = vec![0.0; n_centers];
            for k in 0..n_centers {
                variance[k] = dist_func(
                    &dataset[i],
                    &self.centers[k]);
            }

            let min_cluster = variance
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap();

            if min_cluster != curr_cluster {
                changed.fetch_add(1);
            }

            *cluster = min_cluster;

        });

        return changed.load();
    }

    /// Fits kmeans to dataset with dist function
    pub fn fit(&mut self, dataset: &Vec<Histogram>,
            dist_func: &'static (dyn Fn(&Histogram, &Histogram) -> f32 + Sync)
            ) -> Vec<usize> {

        let start = Instant::now();

        // number of means
        let k = self.centers.len();
        // length of data set
        let n_data: usize = dataset.len();
        // number of features per item in dataset
        let n_bins: usize = dataset[0].len();

        let mut iteration: usize = 0;
        let mut accuracy: f32;

        println!("Fitting {} centers to dataset", k);

        // which cluster each item in dataset is in
        let mut clusters: Vec<usize> = vec![0; n_data];

        loop {

            let changed = self.predict(dataset, &mut clusters, dist_func);

            let mut cluster_elem_counter: Vec<f32> = vec![0.0; k];
            let mut cluster_prob_mass: Vec<Vec<f32>> = vec![vec![0.0; n_bins]; k];

            for i in 0..n_data {
                cluster_elem_counter[clusters[i]] += 1.0;
                for j in 0..n_bins {
                    cluster_prob_mass[clusters[i]][j] +=
                        dataset[i][j];
                }
            }

            // update centers
            for i in 0..k {
                for j in 0..n_bins {
                    if cluster_prob_mass[i][j] > 0.0 {
                        cluster_prob_mass[i][j] /=
                            cluster_elem_counter[i];
                    }
                }
                self.centers[i] = cluster_prob_mass[i].to_owned();
            }

            // print progress to console
            accuracy = changed as f32 / n_data as f32;
            print!("Iteration: {}, epsilon: {:.3}\r", iteration, accuracy);
            io::stdout().flush().unwrap();
            iteration += 1;
            if (accuracy) <= EPSILON {
                break;
            }
        }

        println!("Done.  Took: {}ms", start.elapsed().as_millis());

        return clusters;
    }
}

// used for kmeans ++
pub fn update_min_dists(
        dist_func: &'static (dyn Fn(&Histogram, &Histogram) -> f32 + Sync),
        min_dists: &mut Vec<f32>,
        dataset: &Vec<Histogram>,
        new_center: &Histogram) {

    let mut dist;
    for i in 0..dataset.len() {
        dist = dist_func(&dataset[i], new_center);
        dist = dist * dist;
        if dist < min_dists[i] {
            min_dists[i] = dist;
        }
    }
}

/// Computes the L2 norm distance between two histograms
pub fn l2_dist(a: &Histogram, b: &Histogram) -> f32 {
    let mut sum = 0f32;
    let mut p_sum: f32;
    for i in 0..a.len() {
        p_sum = a[i] - b[i];
        sum += p_sum * p_sum;
    }
    return sum.sqrt();
}
