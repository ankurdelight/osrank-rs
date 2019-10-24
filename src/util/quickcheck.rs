use num_traits::Num;
use quickcheck::{Arbitrary, Gen};
use rand::Rng;

pub type Frequency = u32;

pub fn frequency<G: Rng, A>(g: &mut G, xs: Vec<(Frequency, A)>) -> A {
    let mut tot: u32 = 0;

    for (f, _) in &xs {
        tot += f
    }

    let choice = g.gen_range(1, tot);
    pick(choice, xs)
}

fn pick<A>(n: u32, xs: Vec<(Frequency, A)>) -> A {
    let mut acc = n;

    for (k, x) in xs {
        if acc <= k {
            return x;
        } else {
            acc -= k;
        }
    }

    panic!("QuickCheck.pick used with an empty vector");
}

#[derive(Debug, Clone)]
pub struct Positive<N> {
    pub get_positive: N,
}

impl<N> Arbitrary for Positive<N>
where
    N: Num + Clone + Arbitrary,
{
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let mut x: N = Arbitrary::arbitrary(g);

        while x == N::zero() {
            x = Arbitrary::arbitrary(g);
        }

        Positive { get_positive: x }
    }
}
