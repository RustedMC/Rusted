// SPDX-License-Identifier: LGPL-3.0 -- ported from PowerNukkitX; see NOTICE.md.
//! World features PNX has that came over later: springs, amethyst geodes, dripstone clusters,
//! sculk patches, azalea root systems and the sulfur cave features.

use super::FeatureFn;
use std::collections::HashMap;

mod azalea_roots;
mod dripstone_cluster;
mod geode;
mod sculk_patch;
mod springs;
mod sulfur_pool;
mod sulfur_spike;
mod sulfur_spike_cluster;
mod sulfur_spring_trail;

pub fn register(m: &mut HashMap<&'static str, FeatureFn>) {
    springs::register(m);
    geode::register(m);
    dripstone_cluster::register(m);
    sculk_patch::register(m);
    azalea_roots::register(m);
    sulfur_spike::register(m);
    sulfur_spike_cluster::register(m);
    sulfur_pool::register(m);
    sulfur_spring_trail::register(m);
}
