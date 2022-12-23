use std::cmp::Ordering;

use rand::Rng;
use trust_dns_resolver::lookup::SrvLookupIter;
use trust_dns_resolver::proto::rr::rdata::SRV;

#[derive(Eq, PartialOrd, Clone)]
pub struct SrvRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub target: String,
}

impl From<&SRV> for SrvRecord {
    fn from(value: &SRV) -> Self {
        SrvRecord {
            priority: value.priority(),
            weight: value.weight(),
            port: value.port(),
            target: value.target().to_string(),
        }
    }
}

impl PartialEq for SrvRecord {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Ord for SrvRecord {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

pub struct WeightedSrvMap {
    records: Vec<SrvRecord>,
}

impl Iterator for WeightedSrvMap {
    type Item = SrvRecord;

    fn next(&mut self) -> Option<Self::Item> {
        let mut total_weight = 0;
        for record in self.records.iter() {
            total_weight += record.weight;
        }

        let total_weight = total_weight; // de mut

        let mut rng = rand::thread_rng();
        let v = rng.gen_range(0..=total_weight);
        let mut nv = 0;

        let irm = self.records.len();
        let mut removed = irm; // impossible case

        for (idx, record) in self.records.iter().enumerate() {
            nv += record.weight;
            if nv >= v {
                removed = idx;
                break;
            }
        }

        if removed == irm {
            None
        } else {
            Some(self.records.remove(removed))
        }
    }
}

pub struct PrioritySrvLoader<I: Iterator<Item = WeightedSrvMap>> {
    inner_maps: I,
    current_map: Option<WeightedSrvMap>,
}

impl<I: Iterator<Item = WeightedSrvMap>> Iterator for PrioritySrvLoader<I> {
    type Item = SrvRecord;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.current_map.as_mut() {
            if let Some(record) = current.next() {
                return Some(record);
            }

            self.current_map = None;
            return self.next();
        }

        if let Some(next_map) = self.inner_maps.next() {
            self.current_map = Some(next_map);
            return self.next();
        }

        None
    }
}

impl PrioritySrvLoader<PriorityGroupIter<std::vec::IntoIter<SrvRecord>>> {
    pub fn from_trust_iter(trust_dns_iter: SrvLookupIter) -> Self {
        Self::new(trust_dns_iter.map(Into::into).collect())
    }

    pub fn new(mut records: Vec<SrvRecord>) -> Self {
        records.sort_unstable();

        Self {
            inner_maps: records.into_iter().priority_groupings(),
            current_map: None,
        }
    }
}

pub struct PriorityGroupIter<I: Iterator<Item = SrvRecord>> {
    inner: I,
    n_cache: Option<SrvRecord>,
}

impl<I: Iterator<Item = SrvRecord>> PriorityGroupIter<I> {
    fn pull(&mut self, first_record: SrvRecord) -> WeightedSrvMap {
        let mut records = vec![first_record];
        let priority = records[0].priority;

        while let Some(record) = self.inner.next() {
            if record.priority != priority {
                self.n_cache = Some(record);
                break;
            }

            records.push(record);
        }

        WeightedSrvMap { records }
    }
}

impl<I: Iterator<Item = SrvRecord>> Iterator for PriorityGroupIter<I> {
    type Item = WeightedSrvMap;

    fn next(&mut self) -> Option<Self::Item> {
        match self.n_cache.take() {
            None => self.inner.next().map(|record| self.pull(record)),
            Some(record) => Some(self.pull(record)),
        }
    }
}

pub trait IntoPriorityGroupIter<I: Iterator<Item = SrvRecord>> {
    fn priority_groupings(self) -> PriorityGroupIter<I>;
}

impl<I: Iterator<Item = SrvRecord>> IntoPriorityGroupIter<I> for I {
    fn priority_groupings(self) -> PriorityGroupIter<I> {
        PriorityGroupIter {
            inner: self,
            n_cache: None,
        }
    }
}

pub trait IntoPriorityResolver {
    fn priority_resolver(
        self,
    ) -> PrioritySrvLoader<PriorityGroupIter<std::vec::IntoIter<SrvRecord>>>;
}

impl<'a> IntoPriorityResolver for SrvLookupIter<'a> {
    fn priority_resolver(
        self,
    ) -> PrioritySrvLoader<PriorityGroupIter<std::vec::IntoIter<SrvRecord>>> {
        PrioritySrvLoader::from_trust_iter(self)
    }
}
