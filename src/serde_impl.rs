use std::borrow::Cow;

use super::G1;
use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use poly::{coeff_pos, BivarCommitment};

const ERR_DEG: &str = "commitment degree does not match coefficients";

/// A type with the same content as `BivarCommitment`, but that has not been validated yet.
#[derive(Serialize, Deserialize)]
struct WireBivarCommitment<'a> {
    /// The polynomial's degree in each of the two variables.
    degree: usize,
    /// The commitments to the coefficients.
    #[serde(with = "projective_vec")]
    coeff: Cow<'a, [G1]>,
}

impl Serialize for BivarCommitment {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        WireBivarCommitment {
            degree: self.degree,
            coeff: Cow::Borrowed(&self.coeff),
        }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for BivarCommitment {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let WireBivarCommitment { degree, coeff } = Deserialize::deserialize(d)?;
        if coeff_pos(degree, degree).and_then(|l| l.checked_add(1)) != Some(coeff.len()) {
            return Err(D::Error::custom(ERR_DEG));
        }
        Ok(BivarCommitment {
            degree,
            coeff: coeff.into(),
        })
    }
}

/// Serialization and deserialization of a group element's compressed representation.
pub mod projective {
    use pairing::{CurveAffine, CurveProjective, EncodedPoint};
    use serde::de::Error as DeserializeError;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    const ERR_LEN: &str = "wrong length of deserialized group element";
    const ERR_CODE: &str = "deserialized bytes don't encode a group element";

    pub fn serialize<S, C>(c: &C, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        C: CurveProjective,
    {
        c.into_affine().into_compressed().as_ref().serialize(s)
    }

    pub fn deserialize<'de, D, C>(d: D) -> Result<C, D::Error>
    where
        D: Deserializer<'de>,
        C: CurveProjective,
    {
        let bytes = <Vec<u8>>::deserialize(d)?;
        if bytes.len() != <C::Affine as CurveAffine>::Compressed::size() {
            return Err(D::Error::custom(ERR_LEN));
        }
        let mut compressed = <C::Affine as CurveAffine>::Compressed::empty();
        compressed.as_mut().copy_from_slice(&bytes);
        let to_err = |_| D::Error::custom(ERR_CODE);
        Ok(compressed.into_affine().map_err(to_err)?.into_projective())
    }
}

/// Serialization and deserialization of vectors of projective curve elements.
pub mod projective_vec {
    use std::borrow::Borrow;
    use std::iter::FromIterator;
    use std::marker::PhantomData;

    use pairing::CurveProjective;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::projective;

    /// A wrapper type to facilitate serialization and deserialization of group elements.
    struct CurveWrap<C, B>(B, PhantomData<C>);

    impl<C, B> CurveWrap<C, B> {
        fn new(c: B) -> Self {
            CurveWrap(c, PhantomData)
        }
    }

    impl<C: CurveProjective, B: Borrow<C>> Serialize for CurveWrap<C, B> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            projective::serialize(self.0.borrow(), s)
        }
    }

    impl<'de, C: CurveProjective> Deserialize<'de> for CurveWrap<C, C> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            Ok(CurveWrap::new(projective::deserialize(d)?))
        }
    }

    pub fn serialize<S, C, T>(vec: T, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        C: CurveProjective,
        T: AsRef<[C]>,
    {
        let wrap_vec: Vec<CurveWrap<C, &C>> = vec.as_ref().iter().map(CurveWrap::new).collect();
        wrap_vec.serialize(s)
    }

    pub fn deserialize<'de, D, C, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        C: CurveProjective,
        T: FromIterator<C>,
    {
        let wrap_vec = <Vec<CurveWrap<C, C>>>::deserialize(d)?;
        Ok(wrap_vec.into_iter().map(|CurveWrap(c, _)| c).collect())
    }
}

/// Serialization and deserialization of vectors of field elements.
pub mod field_vec {
    use std::borrow::Borrow;
    use std::marker::PhantomData;

    use pairing::{PrimeField, PrimeFieldRepr};
    use serde::de::Error as DeserializeError;
    use serde::ser::Error as SerializeError;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// A wrapper type to facilitate serialization and deserialization of field elements.
    pub struct FieldWrap<F, B>(B, PhantomData<F>);

    impl<F, B> FieldWrap<F, B> {
        pub fn new(f: B) -> Self {
            FieldWrap(f, PhantomData)
        }
    }

    impl<F> FieldWrap<F, F> {
        pub fn into_inner(self) -> F {
            self.0
        }
    }

    impl<F: PrimeField, B: Borrow<F>> Serialize for FieldWrap<F, B> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut bytes = Vec::new();
            self.0
                .borrow()
                .into_repr()
                .write_be(&mut bytes)
                .map_err(|_| S::Error::custom("failed to write bytes"))?;
            bytes.serialize(s)
        }
    }

    impl<'de, F: PrimeField> Deserialize<'de> for FieldWrap<F, F> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let bytes: Vec<u8> = Deserialize::deserialize(d)?;
            let mut repr = F::zero().into_repr();
            repr.read_be(&bytes[..])
                .map_err(|_| D::Error::custom("failed to write bytes"))?;
            Ok(FieldWrap::new(F::from_repr(repr).map_err(|_| {
                D::Error::custom("invalid field element representation")
            })?))
        }
    }

    pub fn serialize<S, F>(vec: &[F], s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        F: PrimeField,
    {
        let wrap_vec: Vec<FieldWrap<F, &F>> = vec.iter().map(FieldWrap::new).collect();
        wrap_vec.serialize(s)
    }

    pub fn deserialize<'de, D, F>(d: D) -> Result<Vec<F>, D::Error>
    where
        D: Deserializer<'de>,
        F: PrimeField,
    {
        let wrap_vec = <Vec<FieldWrap<F, F>>>::deserialize(d)?;
        Ok(wrap_vec.into_iter().map(|FieldWrap(f, _)| f).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::PEngine;
    use bincode;
    use pairing::Engine;
    use rand::{self, Rng};

    use poly::BivarPoly;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Vecs<E: Engine> {
        #[serde(with = "super::projective_vec")]
        curve_points: Vec<E::G1>,
        #[serde(with = "super::field_vec")]
        field_elements: Vec<E::Fr>,
    }

    impl<E: Engine> PartialEq for Vecs<E> {
        fn eq(&self, other: &Self) -> bool {
            self.curve_points == other.curve_points && self.field_elements == other.field_elements
        }
    }

    #[test]
    fn vecs() {
        let mut rng = rand::thread_rng();
        let vecs: Vecs<PEngine> = Vecs {
            curve_points: rng.gen_iter().take(10).collect(),
            field_elements: rng.gen_iter().take(10).collect(),
        };
        let ser_vecs = bincode::serialize(&vecs).expect("serialize vecs");
        let de_vecs = bincode::deserialize(&ser_vecs).expect("deserialize vecs");
        assert_eq!(vecs, de_vecs);
    }

    #[test]
    fn bivar_commitment() {
        let mut rng = rand::thread_rng();
        for deg in 1..8 {
            let poly = BivarPoly::random(deg, &mut rng);
            let comm = poly.commitment();
            let ser_comm = bincode::serialize(&comm).expect("serialize commitment");
            let de_comm = bincode::deserialize(&ser_comm).expect("deserialize commitment");
            assert_eq!(comm, de_comm);
        }
    }
}
