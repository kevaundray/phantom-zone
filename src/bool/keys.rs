use std::{collections::HashMap, hash::Hash, marker::PhantomData};

use crate::{
    backend::{ModInit, VectorOps},
    lwe::LweSecret,
    pbs::WithShoupRepr,
    random::{NewWithSeed, RandomFillUniformInModulus},
    rgsw::RlweSecret,
    utils::{ToShoup, WithLocal},
    Decryptor, Encryptor, Matrix, MatrixEntity, MatrixMut, MultiPartyDecryptor, RowEntity, RowMut,
};

use super::{parameters, BoolEvaluator, BoolParameters, CiphertextModulus};

trait SinglePartyClientKey {
    type Element;
    fn sk_rlwe(&self) -> &[Self::Element];
    fn sk_lwe(&self) -> &[Self::Element];
}

trait InteractiveMultiPartyClientKey {
    type Element;
    fn sk_rlwe(&self) -> &[Self::Element];
    fn sk_lwe(&self) -> &[Self::Element];
}

trait NonInteractiveMultiPartyClientKey {
    type Element;
    fn sk_rlwe(&self) -> &[Self::Element];
    fn sk_u_rlwe(&self) -> &[Self::Element];
    fn sk_lwe(&self) -> &[Self::Element];
}

/// Client key with RLWE and LWE secrets
#[derive(Clone)]
pub struct ClientKey {
    sk_rlwe: RlweSecret,
    sk_lwe: LweSecret,
}

/// Client key with RLWE and LWE secrets
#[derive(Clone)]
pub struct ThrowMeAwayKey {
    sk_rlwe: RlweSecret,
    sk_u_rlwe: RlweSecret,
    sk_lwe: LweSecret,
}

mod impl_ck {
    use super::*;

    // Client key
    impl ClientKey {
        pub(in super::super) fn new(sk_rlwe: RlweSecret, sk_lwe: LweSecret) -> Self {
            Self { sk_rlwe, sk_lwe }
        }

        pub(in super::super) fn sk_rlwe(&self) -> &RlweSecret {
            &self.sk_rlwe
        }

        pub(in super::super) fn sk_lwe(&self) -> &LweSecret {
            &self.sk_lwe
        }
    }

    // Client key
    impl ThrowMeAwayKey {
        pub(in super::super) fn new(
            sk_rlwe: RlweSecret,
            sk_u_rlwe: RlweSecret,
            sk_lwe: LweSecret,
        ) -> Self {
            Self {
                sk_rlwe,
                sk_u_rlwe,
                sk_lwe,
            }
        }

        pub(in super::super) fn sk_rlwe(&self) -> &RlweSecret {
            &self.sk_rlwe
        }

        pub(in super::super) fn sk_u_rlwe(&self) -> &RlweSecret {
            &self.sk_u_rlwe
        }

        pub(in super::super) fn sk_lwe(&self) -> &LweSecret {
            &self.sk_lwe
        }
    }

    impl Encryptor<bool, Vec<u64>> for ClientKey {
        fn encrypt(&self, m: &bool) -> Vec<u64> {
            BoolEvaluator::with_local(|e| e.sk_encrypt(*m, self))
        }
    }

    impl Decryptor<bool, Vec<u64>> for ClientKey {
        fn decrypt(&self, c: &Vec<u64>) -> bool {
            BoolEvaluator::with_local(|e| e.sk_decrypt(c, self))
        }
    }

    impl MultiPartyDecryptor<bool, Vec<u64>> for ClientKey {
        type DecryptionShare = u64;

        fn gen_decryption_share(&self, c: &Vec<u64>) -> Self::DecryptionShare {
            BoolEvaluator::with_local(|e| e.multi_party_decryption_share(c, &self))
        }

        fn aggregate_decryption_shares(
            &self,
            c: &Vec<u64>,
            shares: &[Self::DecryptionShare],
        ) -> bool {
            BoolEvaluator::with_local(|e| e.multi_party_decrypt(shares, c))
        }
    }
}

/// Public key
pub struct PublicKey<M, Rng, ModOp> {
    key: M,
    _phantom: PhantomData<(Rng, ModOp)>,
}

pub(super) mod impl_pk {
    use super::*;

    impl<M, R, Mo> PublicKey<M, R, Mo> {
        pub(in super::super) fn key(&self) -> &M {
            &self.key
        }
    }

    impl<Rng, ModOp> Encryptor<bool, Vec<u64>> for PublicKey<Vec<Vec<u64>>, Rng, ModOp> {
        fn encrypt(&self, m: &bool) -> Vec<u64> {
            BoolEvaluator::with_local(|e| e.pk_encrypt(&self.key, *m))
        }
    }

    impl<Rng, ModOp> Encryptor<[bool], Vec<Vec<u64>>> for PublicKey<Vec<Vec<u64>>, Rng, ModOp> {
        fn encrypt(&self, m: &[bool]) -> Vec<Vec<u64>> {
            BoolEvaluator::with_local(|e| e.pk_encrypt_batched(&self.key, m))
        }
    }

    impl<
            M: MatrixMut + MatrixEntity,
            Rng: NewWithSeed
                + RandomFillUniformInModulus<[M::MatElement], CiphertextModulus<M::MatElement>>,
            ModOp,
        > From<SeededPublicKey<M::R, Rng::Seed, BoolParameters<M::MatElement>, ModOp>>
        for PublicKey<M, Rng, ModOp>
    where
        <M as Matrix>::R: RowMut,
        M::MatElement: Copy,
    {
        fn from(
            value: SeededPublicKey<M::R, Rng::Seed, BoolParameters<M::MatElement>, ModOp>,
        ) -> Self {
            let mut prng = Rng::new_with_seed(value.seed);

            let mut key = M::zeros(2, value.parameters.rlwe_n().0);
            // sample A
            RandomFillUniformInModulus::random_fill(
                &mut prng,
                value.parameters.rlwe_q(),
                key.get_row_mut(0),
            );
            // Copy over B
            key.get_row_mut(1).copy_from_slice(value.part_b.as_ref());

            PublicKey {
                key,
                _phantom: PhantomData,
            }
        }
    }

    impl<
            M: MatrixMut + MatrixEntity,
            Rng: NewWithSeed
                + RandomFillUniformInModulus<[M::MatElement], CiphertextModulus<M::MatElement>>,
            ModOp: VectorOps<Element = M::MatElement> + ModInit<M = CiphertextModulus<M::MatElement>>,
        >
        From<
            &[CommonReferenceSeededCollectivePublicKeyShare<
                M::R,
                Rng::Seed,
                BoolParameters<M::MatElement>,
            >],
        > for PublicKey<M, Rng, ModOp>
    where
        <M as Matrix>::R: RowMut,
        Rng::Seed: Copy + PartialEq,
        M::MatElement: PartialEq + Copy,
    {
        fn from(
            value: &[CommonReferenceSeededCollectivePublicKeyShare<
                M::R,
                Rng::Seed,
                BoolParameters<M::MatElement>,
            >],
        ) -> Self {
            assert!(value.len() > 0);

            let parameters = &value[0].parameters;
            let mut key = M::zeros(2, parameters.rlwe_n().0);

            // sample A
            let seed = value[0].cr_seed;
            let mut main_rng = Rng::new_with_seed(seed);
            RandomFillUniformInModulus::random_fill(
                &mut main_rng,
                parameters.rlwe_q(),
                key.get_row_mut(0),
            );

            // Sum all Bs
            let rlweq_modop = ModOp::new(parameters.rlwe_q().clone());
            value.iter().for_each(|share_i| {
                assert!(share_i.cr_seed == seed);
                assert!(&share_i.parameters == parameters);

                rlweq_modop.elwise_add_mut(key.get_row_mut(1), share_i.share.as_ref());
            });

            PublicKey {
                key,
                _phantom: PhantomData,
            }
        }
    }
}

/// Seeded public key
struct SeededPublicKey<Ro, S, P, ModOp> {
    part_b: Ro,
    seed: S,
    parameters: P,
    _phantom: PhantomData<ModOp>,
}

mod impl_seeded_pk {
    use super::*;

    impl<R, S, ModOp>
        From<&[CommonReferenceSeededCollectivePublicKeyShare<R, S, BoolParameters<R::Element>>]>
        for SeededPublicKey<R, S, BoolParameters<R::Element>, ModOp>
    where
        ModOp: VectorOps<Element = R::Element> + ModInit<M = CiphertextModulus<R::Element>>,
        S: PartialEq + Clone,
        R: RowMut + RowEntity + Clone,
        R::Element: Clone + PartialEq,
    {
        fn from(
            value: &[CommonReferenceSeededCollectivePublicKeyShare<
                R,
                S,
                BoolParameters<R::Element>,
            >],
        ) -> Self {
            assert!(value.len() > 0);

            let parameters = &value[0].parameters;
            let cr_seed = value[0].cr_seed.clone();

            // Sum all Bs
            let rlweq_modop = ModOp::new(parameters.rlwe_q().clone());
            let mut part_b = value[0].share.clone();
            value.iter().skip(1).for_each(|share_i| {
                assert!(&share_i.cr_seed == &cr_seed);
                assert!(&share_i.parameters == parameters);

                rlweq_modop.elwise_add_mut(part_b.as_mut(), share_i.share.as_ref());
            });

            Self {
                part_b,
                seed: cr_seed,
                parameters: parameters.clone(),
                _phantom: PhantomData,
            }
        }
    }
}

/// CRS seeded collective public key share
pub struct CommonReferenceSeededCollectivePublicKeyShare<Ro, S, P> {
    share: Ro,
    cr_seed: S,
    parameters: P,
}
impl<Ro, S, P> CommonReferenceSeededCollectivePublicKeyShare<Ro, S, P> {
    pub(super) fn new(share: Ro, cr_seed: S, parameters: P) -> Self {
        CommonReferenceSeededCollectivePublicKeyShare {
            share,
            cr_seed,
            parameters,
        }
    }
}

/// CRS seeded Multi-party server key share
pub struct CommonReferenceSeededMultiPartyServerKeyShare<M: Matrix, P, S> {
    rgsw_cts: Vec<M>,
    /// Auto keys. Key corresponding to g^{k} is at index `k`. Key corresponding
    /// to -g is at 0
    auto_keys: HashMap<usize, M>,
    lwe_ksk: M::R,
    /// Common reference seed
    cr_seed: S,
    parameters: P,
}

impl<M: Matrix, P, S> CommonReferenceSeededMultiPartyServerKeyShare<M, P, S> {
    pub(super) fn new(
        rgsw_cts: Vec<M>,
        auto_keys: HashMap<usize, M>,
        lwe_ksk: M::R,
        cr_seed: S,
        parameters: P,
    ) -> Self {
        CommonReferenceSeededMultiPartyServerKeyShare {
            rgsw_cts,
            auto_keys,
            lwe_ksk,
            cr_seed,
            parameters,
        }
    }

    pub(super) fn cr_seed(&self) -> &S {
        &self.cr_seed
    }

    pub(super) fn parameters(&self) -> &P {
        &self.parameters
    }

    pub(super) fn auto_keys(&self) -> &HashMap<usize, M> {
        &self.auto_keys
    }

    pub(super) fn rgsw_cts(&self) -> &[M] {
        &self.rgsw_cts
    }

    pub(super) fn lwe_ksk(&self) -> &M::R {
        &self.lwe_ksk
    }
}

/// CRS seeded MultiParty server key
pub struct SeededMultiPartyServerKey<M: Matrix, S, P> {
    rgsw_cts: Vec<M>,
    /// Auto keys. Key corresponding to g^{k} is at index `k`. Key corresponding
    /// to -g is at 0
    auto_keys: HashMap<usize, M>,
    lwe_ksk: M::R,
    cr_seed: S,
    parameters: P,
}

impl<M: Matrix, S, P> SeededMultiPartyServerKey<M, S, P> {
    pub(super) fn new(
        rgsw_cts: Vec<M>,
        auto_keys: HashMap<usize, M>,
        lwe_ksk: M::R,
        cr_seed: S,
        parameters: P,
    ) -> Self {
        SeededMultiPartyServerKey {
            rgsw_cts,
            auto_keys,
            lwe_ksk,
            cr_seed,
            parameters,
        }
    }

    pub(super) fn rgsw_cts(&self) -> &[M] {
        &self.rgsw_cts
    }
}

/// Seeded single party server key
pub struct SeededSinglePartyServerKey<M: Matrix, P, S> {
    /// Rgsw cts of LWE secret elements
    pub(crate) rgsw_cts: Vec<M>,
    /// Auto keys. Key corresponding to g^{k} is at index `k`. Key corresponding
    /// to -g is at 0
    pub(crate) auto_keys: HashMap<usize, M>,
    /// LWE ksk to key switching LWE ciphertext from RLWE secret to LWE secret
    pub(crate) lwe_ksk: M::R,
    /// Parameters
    pub(crate) parameters: P,
    /// Main seed
    pub(crate) seed: S,
}
impl<M: Matrix, S> SeededSinglePartyServerKey<M, BoolParameters<M::MatElement>, S> {
    pub(super) fn from_raw(
        auto_keys: HashMap<usize, M>,
        rgsw_cts: Vec<M>,
        lwe_ksk: M::R,
        parameters: BoolParameters<M::MatElement>,
        seed: S,
    ) -> Self {
        // sanity checks
        auto_keys.iter().for_each(|v| {
            assert!(
                v.1.dimension()
                    == (
                        parameters.auto_decomposition_count().0,
                        parameters.rlwe_n().0
                    )
            )
        });

        let (part_a_d, part_b_d) = parameters.rlwe_rgsw_decomposition_count();
        rgsw_cts.iter().for_each(|v| {
            assert!(v.dimension() == (part_a_d.0 * 2 + part_b_d.0, parameters.rlwe_n().0))
        });
        assert!(
            lwe_ksk.as_ref().len()
                == (parameters.lwe_decomposition_count().0 * parameters.rlwe_n().0)
        );

        SeededSinglePartyServerKey {
            rgsw_cts,
            auto_keys,
            lwe_ksk,
            parameters,
            seed,
        }
    }
}

/// Server key in evaluation domain
pub(crate) struct ServerKeyEvaluationDomain<M, P, R, N> {
    /// Rgsw cts of LWE secret elements
    rgsw_cts: Vec<M>,
    /// Auto keys. Key corresponding to g^{k} is at index `k`. Key corresponding
    /// to -g is at 0
    galois_keys: HashMap<usize, M>,
    /// LWE ksk to key switching LWE ciphertext from RLWE secret to LWE secret
    lwe_ksk: M,
    parameters: P,
    _phanton: PhantomData<(R, N)>,
}

pub(super) mod impl_server_key_eval_domain {
    use itertools::{izip, Itertools};

    use crate::{
        backend::Modulus,
        bool::{NonInteractiveMultiPartyCrs, SeededNonInteractiveMultiPartyServerKey},
        ntt::{Ntt, NttInit},
        pbs::PbsKey,
    };

    use super::*;

    impl<M, Mod, R, N> ServerKeyEvaluationDomain<M, Mod, R, N> {
        pub(in super::super) fn rgsw_cts(&self) -> &[M] {
            &self.rgsw_cts
        }
    }

    impl<
            M: MatrixMut + MatrixEntity,
            R: RandomFillUniformInModulus<[M::MatElement], CiphertextModulus<M::MatElement>>
                + NewWithSeed,
            N: NttInit<CiphertextModulus<M::MatElement>> + Ntt<Element = M::MatElement>,
        > From<&SeededSinglePartyServerKey<M, BoolParameters<M::MatElement>, R::Seed>>
        for ServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, R, N>
    where
        <M as Matrix>::R: RowMut,
        M::MatElement: Copy,
        R::Seed: Clone,
    {
        fn from(
            value: &SeededSinglePartyServerKey<M, BoolParameters<M::MatElement>, R::Seed>,
        ) -> Self {
            let mut main_prng = R::new_with_seed(value.seed.clone());
            let parameters = &value.parameters;
            let g = parameters.g() as isize;
            let ring_size = value.parameters.rlwe_n().0;
            let lwe_n = value.parameters.lwe_n().0;
            let rlwe_q = value.parameters.rlwe_q();
            let lwq_q = value.parameters.lwe_q();

            let nttop = N::new(rlwe_q, ring_size);

            // galois keys
            let mut auto_keys = HashMap::new();
            let auto_decomp_count = parameters.auto_decomposition_count().0;
            let auto_element_dlogs = parameters.auto_element_dlogs();
            for i in auto_element_dlogs.into_iter() {
                let seeded_auto_key = value.auto_keys.get(&i).unwrap();
                assert!(seeded_auto_key.dimension() == (auto_decomp_count, ring_size));

                let mut data = M::zeros(auto_decomp_count * 2, ring_size);

                // sample RLWE'_A(-s(X^k))
                data.iter_rows_mut().take(auto_decomp_count).for_each(|ri| {
                    RandomFillUniformInModulus::random_fill(&mut main_prng, &rlwe_q, ri.as_mut())
                });

                // copy over RLWE'B_(-s(X^k))
                izip!(
                    data.iter_rows_mut().skip(auto_decomp_count),
                    seeded_auto_key.iter_rows()
                )
                .for_each(|(to_ri, from_ri)| to_ri.as_mut().copy_from_slice(from_ri.as_ref()));

                // Send to Evaluation domain
                data.iter_rows_mut()
                    .for_each(|ri| nttop.forward(ri.as_mut()));

                auto_keys.insert(i, data);
            }

            // RGSW ciphertexts
            let (rlrg_a_decomp, rlrg_b_decomp) = parameters.rlwe_rgsw_decomposition_count();
            let rgsw_cts = value
                .rgsw_cts
                .iter()
                .map(|seeded_rgsw_si| {
                    assert!(
                        seeded_rgsw_si.dimension()
                            == (rlrg_a_decomp.0 * 2 + rlrg_b_decomp.0, ring_size)
                    );

                    let mut data = M::zeros(rlrg_a_decomp.0 * 2 + rlrg_b_decomp.0 * 2, ring_size);

                    // copy over RLWE'(-sm)
                    izip!(
                        data.iter_rows_mut().take(rlrg_a_decomp.0 * 2),
                        seeded_rgsw_si.iter_rows().take(rlrg_a_decomp.0 * 2)
                    )
                    .for_each(|(to_ri, from_ri)| to_ri.as_mut().copy_from_slice(from_ri.as_ref()));

                    // sample RLWE'_A(m)
                    data.iter_rows_mut()
                        .skip(rlrg_a_decomp.0 * 2)
                        .take(rlrg_b_decomp.0)
                        .for_each(|ri| {
                            RandomFillUniformInModulus::random_fill(
                                &mut main_prng,
                                &rlwe_q,
                                ri.as_mut(),
                            )
                        });

                    // copy over RLWE'_B(m)
                    izip!(
                        data.iter_rows_mut()
                            .skip(rlrg_a_decomp.0 * 2 + rlrg_b_decomp.0),
                        seeded_rgsw_si.iter_rows().skip(rlrg_a_decomp.0 * 2)
                    )
                    .for_each(|(to_ri, from_ri)| to_ri.as_mut().copy_from_slice(from_ri.as_ref()));

                    // send polynomials to evaluation domain
                    data.iter_rows_mut()
                        .for_each(|ri| nttop.forward(ri.as_mut()));

                    data
                })
                .collect_vec();

            // LWE ksk
            let lwe_ksk = {
                let d = parameters.lwe_decomposition_count().0;
                assert!(value.lwe_ksk.as_ref().len() == d * ring_size);

                let mut data = M::zeros(d * ring_size, lwe_n + 1);
                izip!(data.iter_rows_mut(), value.lwe_ksk.as_ref().iter()).for_each(
                    |(lwe_i, bi)| {
                        RandomFillUniformInModulus::random_fill(
                            &mut main_prng,
                            &lwq_q,
                            &mut lwe_i.as_mut()[1..],
                        );
                        lwe_i.as_mut()[0] = *bi;
                    },
                );

                data
            };

            ServerKeyEvaluationDomain {
                rgsw_cts,
                galois_keys: auto_keys,
                lwe_ksk,
                parameters: parameters.clone(),
                _phanton: PhantomData,
            }
        }
    }

    impl<
            M: MatrixMut + MatrixEntity,
            Rng: NewWithSeed,
            N: NttInit<CiphertextModulus<M::MatElement>> + Ntt<Element = M::MatElement>,
        > From<&SeededMultiPartyServerKey<M, Rng::Seed, BoolParameters<M::MatElement>>>
        for ServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, Rng, N>
    where
        <M as Matrix>::R: RowMut,
        Rng::Seed: Copy,
        Rng: RandomFillUniformInModulus<[M::MatElement], CiphertextModulus<M::MatElement>>,
        M::MatElement: Copy,
    {
        fn from(
            value: &SeededMultiPartyServerKey<M, Rng::Seed, BoolParameters<M::MatElement>>,
        ) -> Self {
            let g = value.parameters.g() as isize;
            let rlwe_n = value.parameters.rlwe_n().0;
            let lwe_n = value.parameters.lwe_n().0;
            let rlwe_q = value.parameters.rlwe_q();
            let lwe_q = value.parameters.lwe_q();

            let mut main_prng = Rng::new_with_seed(value.cr_seed);

            let rlwe_nttop = N::new(rlwe_q, rlwe_n);

            // auto keys
            let mut auto_keys = HashMap::new();
            let auto_d_count = value.parameters.auto_decomposition_count().0;
            let auto_element_dlogs = value.parameters.auto_element_dlogs();
            for i in auto_element_dlogs.into_iter() {
                let mut key = M::zeros(auto_d_count * 2, rlwe_n);

                // sample a
                key.iter_rows_mut().take(auto_d_count).for_each(|ri| {
                    RandomFillUniformInModulus::random_fill(&mut main_prng, &rlwe_q, ri.as_mut())
                });

                let key_part_b = value.auto_keys.get(&i).unwrap();
                assert!(key_part_b.dimension() == (auto_d_count, rlwe_n));
                izip!(
                    key.iter_rows_mut().skip(auto_d_count),
                    key_part_b.iter_rows()
                )
                .for_each(|(to_ri, from_ri)| {
                    to_ri.as_mut().copy_from_slice(from_ri.as_ref());
                });

                // send to evaluation domain
                key.iter_rows_mut()
                    .for_each(|ri| rlwe_nttop.forward(ri.as_mut()));

                auto_keys.insert(i, key);
            }

            // rgsw cts
            let (rlrg_d_a, rlrg_d_b) = value.parameters.rlwe_rgsw_decomposition_count();
            let rgsw_ct_out = rlrg_d_a.0 * 2 + rlrg_d_b.0 * 2;
            let rgsw_cts = value
                .rgsw_cts
                .iter()
                .map(|ct_i_in| {
                    assert!(ct_i_in.dimension() == (rgsw_ct_out, rlwe_n));
                    let mut eval_ct_i_out = M::zeros(rgsw_ct_out, rlwe_n);

                    izip!(eval_ct_i_out.iter_rows_mut(), ct_i_in.iter_rows()).for_each(
                        |(to_ri, from_ri)| {
                            to_ri.as_mut().copy_from_slice(from_ri.as_ref());
                            rlwe_nttop.forward(to_ri.as_mut());
                        },
                    );

                    eval_ct_i_out
                })
                .collect_vec();

            // lwe ksk
            let d_lwe = value.parameters.lwe_decomposition_count().0;
            let mut lwe_ksk = M::zeros(rlwe_n * d_lwe, lwe_n + 1);
            izip!(lwe_ksk.iter_rows_mut(), value.lwe_ksk.as_ref().iter()).for_each(
                |(lwe_i, bi)| {
                    RandomFillUniformInModulus::random_fill(
                        &mut main_prng,
                        &lwe_q,
                        &mut lwe_i.as_mut()[1..],
                    );
                    lwe_i.as_mut()[0] = *bi;
                },
            );

            ServerKeyEvaluationDomain {
                rgsw_cts,
                galois_keys: auto_keys,
                lwe_ksk,
                parameters: value.parameters.clone(),
                _phanton: PhantomData,
            }
        }
    }

    impl<M: Matrix, P, R, N> PbsKey for ServerKeyEvaluationDomain<M, P, R, N> {
        type AutoKey = M;
        type LweKskKey = M;
        type RgswCt = M;

        fn galois_key_for_auto(&self, k: usize) -> &Self::AutoKey {
            self.galois_keys.get(&k).unwrap()
        }
        fn rgsw_ct_lwe_si(&self, si: usize) -> &Self::RgswCt {
            &self.rgsw_cts[si]
        }

        fn lwe_ksk(&self) -> &Self::LweKskKey {
            &self.lwe_ksk
        }
    }
}

pub(crate) struct NonInteractiveServerKeyEvaluationDomain<M, P, R, N> {
    /// RGSW ciphertexts ideal lwe secret key elements under ideal rlwe secret
    rgsw_cts: Vec<M>,
    /// Automorphism keys under ideal rlwe secret
    auto_keys: HashMap<usize, M>,
    /// LWE key switching key from Q -> Q_{ks}
    lwe_ksk: M,
    /// Key switching key from user j to ideal secret key s. User j's ksk is at
    /// j'th element
    ui_to_s_ksks: Vec<M>,
    parameters: P,
    _phanton: PhantomData<(R, N)>,
}

pub(super) mod impl_non_interactive_server_key_eval_domain {
    use itertools::{izip, Itertools};

    use crate::{bool::NonInteractiveMultiPartyCrs, random::RandomFill, Ntt, NttInit};

    use super::*;

    impl<M, Rng, N>
        From<
            SeededNonInteractiveMultiPartyServerKey<
                M,
                NonInteractiveMultiPartyCrs<Rng::Seed>,
                BoolParameters<M::MatElement>,
            >,
        > for NonInteractiveServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, Rng, N>
    where
        M: MatrixMut + MatrixEntity + Clone,
        Rng: NewWithSeed
            + RandomFillUniformInModulus<[M::MatElement], CiphertextModulus<M::MatElement>>
            + RandomFill<<Rng as NewWithSeed>::Seed>,
        N: Ntt<Element = M::MatElement> + NttInit<CiphertextModulus<M::MatElement>>,
        M::R: RowMut,
        M::MatElement: Copy,
        Rng::Seed: Clone + Copy + Default,
    {
        fn from(
            value: SeededNonInteractiveMultiPartyServerKey<
                M,
                NonInteractiveMultiPartyCrs<Rng::Seed>,
                BoolParameters<M::MatElement>,
            >,
        ) -> Self {
            let rlwe_nttop = N::new(value.parameters.rlwe_q(), value.parameters.rlwe_n().0);
            let ring_size = value.parameters.rlwe_n().0;

            // RGSW cts
            // copy over rgsw cts and send to evaluation domain
            let mut rgsw_cts = value.rgsw_cts.clone();
            rgsw_cts.iter_mut().for_each(|c| {
                c.iter_rows_mut()
                    .for_each(|ri| rlwe_nttop.forward(ri.as_mut()))
            });

            // Auto keys
            // populate pseudo random part of auto keys. Then send auto keys to
            // evaluation domain
            let mut auto_keys = HashMap::new();
            let auto_seed = value.cr_seed.auto_keys_cts_seed::<Rng>();
            let mut auto_prng = Rng::new_with_seed(auto_seed);
            let auto_element_dlogs = value.parameters.auto_element_dlogs();
            let d_auto = value.parameters.auto_decomposition_count().0;
            auto_element_dlogs.iter().for_each(|el| {
                let auto_part_b = value
                    .auto_keys
                    .get(el)
                    .expect(&format!("Auto key for element g^{el} not found"));

                assert!(auto_part_b.dimension() == (d_auto, ring_size));

                let mut auto_ct = M::zeros(d_auto, ring_size);

                // sample part A
                auto_ct.iter_rows_mut().take(d_auto).for_each(|ri| {
                    RandomFillUniformInModulus::random_fill(
                        &mut auto_prng,
                        value.parameters.rlwe_q(),
                        ri.as_mut(),
                    )
                });

                // Copy over part B
                izip!(
                    auto_ct.iter_rows_mut().skip(d_auto),
                    auto_part_b.iter_rows()
                )
                .for_each(|(to_ri, from_ri)| to_ri.as_mut().copy_from_slice(from_ri.as_ref()));

                // send to evaluation domain
                auto_ct
                    .iter_rows_mut()
                    .for_each(|r| rlwe_nttop.forward(r.as_mut()));

                auto_keys.insert(*el, auto_ct);
            });

            // LWE ksk
            // populate pseudo random part of lwe ciphertexts in ksk and copy over part b
            // elements
            let lwe_ksk_seed = value.cr_seed.lwe_ksk_cts_seed::<Rng>();
            let mut lwe_ksk_prng = Rng::new_with_seed(lwe_ksk_seed);
            let mut lwe_ksk = M::zeros(
                value.parameters.lwe_decomposition_count().0 * ring_size,
                value.parameters.lwe_n().0 + 1,
            );
            lwe_ksk.iter_rows_mut().for_each(|ri| {
                // first element is resereved for part b. Only sample a_is in the rest
                RandomFillUniformInModulus::random_fill(
                    &mut lwe_ksk_prng,
                    value.parameters.lwe_q(),
                    &mut ri.as_mut()[1..],
                )
            });
            // copy over part bs
            izip!(value.lwe_ksk.as_ref().iter(), lwe_ksk.iter_rows_mut()).for_each(
                |(b_el, lwe_ct)| {
                    lwe_ct.as_mut()[0] = *b_el;
                },
            );

            // u_i to s ksk
            let d_uitos = value
                .parameters
                .non_interactive_ui_to_s_key_switch_decomposition_count()
                .0;
            let total_users = *value.ui_to_s_ksks_key_order.iter().max().unwrap();
            let ui_to_s_ksks = (0..total_users)
                .map(|user_index| {
                    let user_i_seed = value.cr_seed.ui_to_s_ks_seed_for_user_i::<Rng>(user_index);
                    let mut prng = Rng::new_with_seed(user_i_seed);

                    let mut ksk_ct = M::zeros(d_uitos * 2, ring_size);

                    ksk_ct.iter_rows_mut().take(d_uitos).for_each(|r| {
                        RandomFillUniformInModulus::random_fill(
                            &mut prng,
                            value.parameters.rlwe_q(),
                            r.as_mut(),
                        );
                    });

                    let incoming_ksk_partb_ref =
                        &value.ui_to_s_ksks[value.ui_to_s_ksks_key_order[user_index]];
                    assert!(ksk_ct.dimension() == (d_uitos, ring_size));
                    izip!(
                        ksk_ct.iter_rows_mut().skip(d_uitos),
                        incoming_ksk_partb_ref.iter_rows()
                    )
                    .for_each(|(to_ri, from_ri)| {
                        to_ri.as_mut().copy_from_slice(from_ri.as_ref());
                    });

                    ksk_ct
                        .iter_rows_mut()
                        .for_each(|r| rlwe_nttop.forward(r.as_mut()));
                    ksk_ct
                })
                .collect_vec();

            NonInteractiveServerKeyEvaluationDomain {
                rgsw_cts,
                auto_keys,
                lwe_ksk,
                ui_to_s_ksks,
                parameters: value.parameters.clone(),
                _phanton: PhantomData,
            }
        }
    }
}

pub struct SeededNonInteractiveMultiPartyServerKey<M: Matrix, S, P> {
    /// u_i to s key switching keys in random order
    ui_to_s_ksks: Vec<M>,
    /// Defines order for u_i to s key switchin keys by storing the index of
    /// user j's ksk in `ui_to_s_ksks` at index `j`. Find user j's u_i to s ksk
    /// at `ui_to_s_ksks[ui_to_s_ksks_key_order[j]]`
    ui_to_s_ksks_key_order: Vec<usize>,
    /// RGSW ciphertets
    rgsw_cts: Vec<M>,
    auto_keys: HashMap<usize, M>,
    lwe_ksk: M::R,
    cr_seed: S,
    parameters: P,
}

impl<M: Matrix, S, P> SeededNonInteractiveMultiPartyServerKey<M, S, P> {
    pub(super) fn new(
        ui_to_s_ksks: Vec<M>,
        ui_to_s_ksks_key_order: Vec<usize>,
        rgsw_cts: Vec<M>,
        auto_keys: HashMap<usize, M>,
        lwe_ksk: M::R,
        cr_seed: S,
        parameters: P,
    ) -> Self {
        Self {
            ui_to_s_ksks,
            ui_to_s_ksks_key_order,
            rgsw_cts,
            auto_keys,
            lwe_ksk,
            cr_seed,
            parameters,
        }
    }
}

pub(crate) struct ShoupNonInteractiveServerKeyEvaluationDomain<M> {
    /// RGSW ciphertexts ideal lwe secret key elements under ideal rlwe secret
    rgsw_cts: Vec<NormalAndShoup<M>>,
    /// Automorphism keys under ideal rlwe secret
    auto_keys: HashMap<usize, NormalAndShoup<M>>,
    /// LWE key switching key from Q -> Q_{ks}
    lwe_ksk: M,
    /// Key switching key from user j to ideal secret key s. User j's ksk is at
    /// j'th element
    ui_to_s_ksks: Vec<NormalAndShoup<M>>,
}

mod impl_shoup_non_interactive_server_key_eval_domain {
    use itertools::Itertools;
    use num_traits::{FromPrimitive, PrimInt, ToPrimitive};

    use super::*;
    use crate::{backend::Modulus, pbs::PbsKey};

    impl<M: Matrix + ToShoup<Modulus = M::MatElement>, R, N>
        From<NonInteractiveServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, R, N>>
        for ShoupNonInteractiveServerKeyEvaluationDomain<M>
    where
        M::MatElement: FromPrimitive + ToPrimitive + PrimInt,
    {
        fn from(
            value: NonInteractiveServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, R, N>,
        ) -> Self {
            let rlwe_q = value.parameters.rlwe_q().q().unwrap();

            let rgsw_cts = value
                .rgsw_cts
                .into_iter()
                .map(|m| NormalAndShoup::new_with_modulus(m, rlwe_q))
                .collect_vec();

            let mut auto_keys = HashMap::new();
            value.auto_keys.into_iter().for_each(|(k, v)| {
                auto_keys.insert(k, NormalAndShoup::new_with_modulus(v, rlwe_q));
            });

            let ui_to_s_ksks = value
                .ui_to_s_ksks
                .into_iter()
                .map(|m| NormalAndShoup::new_with_modulus(m, rlwe_q))
                .collect_vec();

            Self {
                rgsw_cts,
                auto_keys,
                lwe_ksk: value.lwe_ksk,
                ui_to_s_ksks,
            }
        }
    }

    impl<M: Matrix> PbsKey for ShoupNonInteractiveServerKeyEvaluationDomain<M> {
        type AutoKey = NormalAndShoup<M>;
        type LweKskKey = M;
        type RgswCt = NormalAndShoup<M>;

        fn galois_key_for_auto(&self, k: usize) -> &Self::AutoKey {
            self.auto_keys.get(&k).unwrap()
        }
        fn rgsw_ct_lwe_si(&self, si: usize) -> &Self::RgswCt {
            &self.rgsw_cts[si]
        }

        fn lwe_ksk(&self) -> &Self::LweKskKey {
            &self.lwe_ksk
        }
    }
}

/// Server key in evaluation domain with Shoup representations
pub(crate) struct ShoupServerKeyEvaluationDomain<M> {
    /// Rgsw cts of LWE secret elements
    rgsw_cts: Vec<NormalAndShoup<M>>,
    /// Auto keys. Key corresponding to g^{k} is at index `k`. Key corresponding
    /// to -g is at 0
    galois_keys: HashMap<usize, NormalAndShoup<M>>,
    /// LWE ksk to key switching LWE ciphertext from RLWE secret to LWE secret
    lwe_ksk: M,
}

mod shoup_server_key_eval_domain {
    use itertools::{izip, Itertools};
    use num_traits::{FromPrimitive, PrimInt};

    use crate::{backend::Modulus, pbs::PbsKey};

    use super::*;

    impl<M: MatrixMut + MatrixEntity + ToShoup<Modulus = M::MatElement>, R, N>
        From<ServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, R, N>>
        for ShoupServerKeyEvaluationDomain<M>
    where
        <M as Matrix>::R: RowMut,
        M::MatElement: PrimInt + FromPrimitive,
    {
        fn from(value: ServerKeyEvaluationDomain<M, BoolParameters<M::MatElement>, R, N>) -> Self {
            let q = value.parameters.rlwe_q().q().unwrap();
            // Rgsw ciphertexts
            let rgsw_cts = value
                .rgsw_cts
                .into_iter()
                .map(|ct| NormalAndShoup::new_with_modulus(ct, q))
                .collect_vec();

            let mut auto_keys = HashMap::new();
            value.galois_keys.into_iter().for_each(|(index, key)| {
                auto_keys.insert(index, NormalAndShoup::new_with_modulus(key, q));
            });

            Self {
                rgsw_cts,
                galois_keys: auto_keys,
                lwe_ksk: value.lwe_ksk,
            }
        }
    }

    impl<M: Matrix> PbsKey for ShoupServerKeyEvaluationDomain<M> {
        type AutoKey = NormalAndShoup<M>;
        type LweKskKey = M;
        type RgswCt = NormalAndShoup<M>;

        fn galois_key_for_auto(&self, k: usize) -> &Self::AutoKey {
            self.galois_keys.get(&k).unwrap()
        }
        fn rgsw_ct_lwe_si(&self, si: usize) -> &Self::RgswCt {
            &self.rgsw_cts[si]
        }

        fn lwe_ksk(&self) -> &Self::LweKskKey {
            &self.lwe_ksk
        }
    }
}

/// Stores normal and shoup representation of Matrix elements (Normal, Shoup)
pub(crate) struct NormalAndShoup<M>(M, M);

impl<M: ToShoup> NormalAndShoup<M> {
    fn new_with_modulus(value: M, modulus: <M as ToShoup>::Modulus) -> Self {
        let value_shoup = M::to_shoup(&value, modulus);
        NormalAndShoup(value, value_shoup)
    }
}

impl<M> AsRef<M> for NormalAndShoup<M> {
    fn as_ref(&self) -> &M {
        &self.0
    }
}

impl<M> WithShoupRepr for NormalAndShoup<M> {
    type M = M;
    fn shoup_repr(&self) -> &Self::M {
        &self.1
    }
}
