use core::marker::PhantomData;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

use num_traits::Zero;
#[cfg(feature = "random")]
use rand::distributions::uniform::{SampleBorrow, SampleUniform, Uniform, UniformSampler};
#[cfg(feature = "random")]
use rand::distributions::{Distribution, Standard};
#[cfg(feature = "random")]
use rand::Rng;

use crate::{
    clamp, clamp_assign, clamp_min_assign,
    color_difference::{get_ciede_difference, ColorDifference},
    contrast_ratio,
    convert::FromColorUnclamped,
    float::Float,
    from_f64,
    white_point::{WhitePoint, D65},
    Alpha, Clamp, ClampAssign, ComponentWise, FloatComponent, FromF64, GetHue, IsWithinBounds,
    LabHue, Lch, Lighten, LightenAssign, Mix, MixAssign, RelativeContrast, Xyz,
};

/// CIE L\*a\*b\* (CIELAB) with an alpha component. See the [`Laba`
/// implementation in `Alpha`](crate::Alpha#Laba).
pub type Laba<Wp = D65, T = f32> = Alpha<Lab<Wp, T>, T>;

/// The CIE L\*a\*b\* (CIELAB) color space.
///
/// CIE L\*a\*b\* is a device independent color space which includes all
/// perceivable colors. It's sometimes used to convert between other color
/// spaces, because of its ability to represent all of their colors, and
/// sometimes in color manipulation, because of its perceptual uniformity. This
/// means that the perceptual difference between two colors is equal to their
/// numerical difference.
///
/// The parameters of L\*a\*b\* are quite different, compared to many other
/// color spaces, so manipulating them manually may be unintuitive.
#[derive(Debug, ArrayCast, FromColorUnclamped, WithAlpha)]
#[cfg_attr(feature = "serializing", derive(Serialize, Deserialize))]
#[palette(
    palette_internal,
    white_point = "Wp",
    component = "T",
    skip_derives(Xyz, Lab, Lch)
)]
#[repr(C)]
pub struct Lab<Wp = D65, T = f32> {
    /// L\* is the lightness of the color. 0.0 gives absolute black and 100
    /// give the brightest white.
    pub l: T,

    /// a\* goes from red at -128 to green at 127.
    pub a: T,

    /// b\* goes from yellow at -128 to blue at 127.
    pub b: T,

    /// The white point associated with the color's illuminant and observer.
    /// D65 for 2 degree observer is used by default.
    #[cfg_attr(feature = "serializing", serde(skip))]
    #[palette(unsafe_zero_sized)]
    pub white_point: PhantomData<Wp>,
}

impl<Wp, T> Copy for Lab<Wp, T> where T: Copy {}

impl<Wp, T> Clone for Lab<Wp, T>
where
    T: Clone,
{
    fn clone(&self) -> Lab<Wp, T> {
        Lab {
            l: self.l.clone(),
            a: self.a.clone(),
            b: self.b.clone(),
            white_point: PhantomData,
        }
    }
}

impl<Wp, T> Lab<Wp, T> {
    /// Create a CIE L\*a\*b\* color.
    pub const fn new(l: T, a: T, b: T) -> Lab<Wp, T> {
        Lab {
            l,
            a,
            b,
            white_point: PhantomData,
        }
    }

    /// Convert to a `(L\*, a\*, b\*)` tuple.
    pub fn into_components(self) -> (T, T, T) {
        (self.l, self.a, self.b)
    }

    /// Convert from a `(L\*, a\*, b\*)` tuple.
    pub fn from_components((l, a, b): (T, T, T)) -> Self {
        Self::new(l, a, b)
    }
}

impl<Wp, T> Lab<Wp, T>
where
    T: Zero + FromF64,
{
    /// Return the `l` value minimum.
    pub fn min_l() -> T {
        T::zero()
    }

    /// Return the `l` value maximum.
    pub fn max_l() -> T {
        from_f64(100.0)
    }

    /// Return the `a` value minimum.
    pub fn min_a() -> T {
        from_f64(-128.0)
    }

    /// Return the `a` value maximum.
    pub fn max_a() -> T {
        from_f64(127.0)
    }

    /// Return the `b` value minimum.
    pub fn min_b() -> T {
        from_f64(-128.0)
    }

    /// Return the `b` value maximum.
    pub fn max_b() -> T {
        from_f64(127.0)
    }
}

///<span id="Laba"></span>[`Laba`](crate::Laba) implementations.
impl<Wp, T, A> Alpha<Lab<Wp, T>, A> {
    /// Create a CIE L\*a\*b\* with transparency.
    pub const fn new(l: T, a: T, b: T, alpha: A) -> Self {
        Alpha {
            color: Lab::new(l, a, b),
            alpha,
        }
    }

    /// Convert to a `(L\*, a\*, b\*, alpha)` tuple.
    pub fn into_components(self) -> (T, T, T, A) {
        (self.color.l, self.color.a, self.color.b, self.alpha)
    }

    /// Convert from a `(L\*, a\*, b\*, alpha)` tuple.
    pub fn from_components((l, a, b, alpha): (T, T, T, A)) -> Self {
        Self::new(l, a, b, alpha)
    }
}

impl<Wp, T> FromColorUnclamped<Lab<Wp, T>> for Lab<Wp, T> {
    fn from_color_unclamped(color: Lab<Wp, T>) -> Self {
        color
    }
}

impl<Wp, T> FromColorUnclamped<Xyz<Wp, T>> for Lab<Wp, T>
where
    Wp: WhitePoint<T>,
    T: FloatComponent,
{
    fn from_color_unclamped(color: Xyz<Wp, T>) -> Self {
        let Xyz {
            mut x,
            mut y,
            mut z,
            ..
        } = color / Wp::get_xyz().with_white_point();

        fn convert<T: FloatComponent>(c: T) -> T {
            let epsilon = from_f64::<T>(6.0 / 29.0).powi(3);
            let kappa: T = from_f64(841.0 / 108.0);
            let delta: T = from_f64(4.0 / 29.0);
            if c > epsilon {
                c.cbrt()
            } else {
                (kappa * c) + delta
            }
        }

        x = convert(x);
        y = convert(y);
        z = convert(z);

        Lab {
            l: ((y * from_f64(116.0)) - from_f64(16.0)),
            a: ((x - y) * from_f64(500.0)),
            b: ((y - z) * from_f64(200.0)),
            white_point: PhantomData,
        }
    }
}

impl<Wp, T> FromColorUnclamped<Lch<Wp, T>> for Lab<Wp, T>
where
    T: FloatComponent,
{
    fn from_color_unclamped(color: Lch<Wp, T>) -> Self {
        Lab {
            l: color.l,
            a: color.chroma.max(T::zero()) * color.hue.to_radians().cos(),
            b: color.chroma.max(T::zero()) * color.hue.to_radians().sin(),
            white_point: PhantomData,
        }
    }
}

impl<Wp, T> From<(T, T, T)> for Lab<Wp, T> {
    fn from(components: (T, T, T)) -> Self {
        Self::from_components(components)
    }
}

impl<Wp, T> From<Lab<Wp, T>> for (T, T, T) {
    fn from(color: Lab<Wp, T>) -> (T, T, T) {
        color.into_components()
    }
}

impl<Wp, T, A> From<(T, T, T, A)> for Alpha<Lab<Wp, T>, A> {
    fn from(components: (T, T, T, A)) -> Self {
        Self::from_components(components)
    }
}

impl<Wp, T, A> From<Alpha<Lab<Wp, T>, A>> for (T, T, T, A) {
    fn from(color: Alpha<Lab<Wp, T>, A>) -> (T, T, T, A) {
        color.into_components()
    }
}

impl<Wp, T> IsWithinBounds for Lab<Wp, T>
where
    T: Zero + FromF64 + PartialOrd,
{
    #[rustfmt::skip]
    #[inline]
    fn is_within_bounds(&self) -> bool {
        self.l >= Self::min_l() && self.l <= Self::max_l() &&
        self.a >= Self::min_a() && self.a <= Self::max_a() &&
        self.b >= Self::min_b() && self.b <= Self::max_b()
    }
}

impl<Wp, T> Clamp for Lab<Wp, T>
where
    T: Zero + FromF64 + PartialOrd,
{
    #[inline]
    fn clamp(self) -> Self {
        Self::new(
            clamp(self.l, Self::min_l(), Self::max_l()),
            clamp(self.a, Self::min_a(), Self::max_a()),
            clamp(self.b, Self::min_b(), Self::max_b()),
        )
    }
}

impl<Wp, T> ClampAssign for Lab<Wp, T>
where
    T: Zero + FromF64 + PartialOrd,
{
    #[inline]
    fn clamp_assign(&mut self) {
        clamp_assign(&mut self.l, Self::min_l(), Self::max_l());
        clamp_assign(&mut self.a, Self::min_a(), Self::max_a());
        clamp_assign(&mut self.b, Self::min_b(), Self::max_b());
    }
}

impl<Wp, T> Mix for Lab<Wp, T>
where
    T: FloatComponent,
{
    type Scalar = T;

    #[inline]
    fn mix(self, other: Self, factor: T) -> Self {
        let factor = clamp(factor, T::zero(), T::one());
        self + (other - self) * factor
    }
}

impl<Wp, T> MixAssign for Lab<Wp, T>
where
    T: FloatComponent + AddAssign,
{
    type Scalar = T;

    #[inline]
    fn mix_assign(&mut self, other: Self, factor: T) {
        let factor = clamp(factor, T::zero(), T::one());
        *self += (other - *self) * factor;
    }
}

impl<Wp, T> Lighten for Lab<Wp, T>
where
    T: FloatComponent,
{
    type Scalar = T;

    #[inline]
    fn lighten(self, factor: T) -> Self {
        let difference = if factor >= T::zero() {
            Self::max_l() - self.l
        } else {
            self.l
        };

        let delta = difference.max(T::zero()) * factor;

        Lab {
            l: (self.l + delta).max(Self::min_l()),
            a: self.a,
            b: self.b,
            white_point: PhantomData,
        }
    }

    #[inline]
    fn lighten_fixed(self, amount: T) -> Self {
        Lab {
            l: (self.l + Self::max_l() * amount).max(Self::min_l()),
            a: self.a,
            b: self.b,
            white_point: PhantomData,
        }
    }
}

impl<Wp, T> LightenAssign for Lab<Wp, T>
where
    T: FloatComponent + AddAssign,
{
    type Scalar = T;

    #[inline]
    fn lighten_assign(&mut self, factor: T) {
        let difference = if factor >= T::zero() {
            Self::max_l() - self.l
        } else {
            self.l
        };

        self.l += difference.max(T::zero()) * factor;
        clamp_min_assign(&mut self.l, Self::min_l());
    }

    #[inline]
    fn lighten_fixed_assign(&mut self, amount: T) {
        self.l += Self::max_l() * amount;
        clamp_min_assign(&mut self.l, Self::min_l());
    }
}

impl<Wp, T> GetHue for Lab<Wp, T>
where
    T: FloatComponent,
{
    type Hue = LabHue<T>;

    fn get_hue(&self) -> Option<LabHue<T>> {
        if self.a == T::zero() && self.b == T::zero() {
            None
        } else {
            Some(LabHue::from_radians(self.b.atan2(self.a)))
        }
    }
}

impl<Wp, T> ColorDifference for Lab<Wp, T>
where
    T: Float + FromF64,
{
    type Scalar = T;

    #[inline]
    fn get_color_difference(self, other: Lab<Wp, T>) -> Self::Scalar {
        get_ciede_difference(self.into(), other.into())
    }
}

impl<Wp, T> ComponentWise for Lab<Wp, T>
where
    T: Clone,
{
    type Scalar = T;

    fn component_wise<F: FnMut(T, T) -> T>(&self, other: &Lab<Wp, T>, mut f: F) -> Lab<Wp, T> {
        Lab {
            l: f(self.l.clone(), other.l.clone()),
            a: f(self.a.clone(), other.a.clone()),
            b: f(self.b.clone(), other.b.clone()),
            white_point: PhantomData,
        }
    }

    fn component_wise_self<F: FnMut(T) -> T>(&self, mut f: F) -> Lab<Wp, T> {
        Lab {
            l: f(self.l.clone()),
            a: f(self.a.clone()),
            b: f(self.b.clone()),
            white_point: PhantomData,
        }
    }
}

impl<Wp, T> Default for Lab<Wp, T>
where
    T: Zero,
{
    fn default() -> Lab<Wp, T> {
        Lab::new(T::zero(), T::zero(), T::zero())
    }
}

impl_color_add!(Lab<Wp, T>, [l, a, b], white_point);
impl_color_sub!(Lab<Wp, T>, [l, a, b], white_point);
impl_color_mul!(Lab<Wp, T>, [l, a, b], white_point);
impl_color_div!(Lab<Wp, T>, [l, a, b], white_point);

impl_array_casts!(Lab<Wp, T>, [T; 3]);

impl<Wp, T> RelativeContrast for Lab<Wp, T>
where
    Wp: WhitePoint<T>,
    T: FloatComponent,
{
    type Scalar = T;

    #[inline]
    fn get_contrast_ratio(self, other: Self) -> T {
        use crate::FromColor;

        let xyz1 = Xyz::from_color(self);
        let xyz2 = Xyz::from_color(other);

        contrast_ratio(xyz1.y, xyz2.y)
    }
}

#[cfg(feature = "random")]
impl<Wp, T> Distribution<Lab<Wp, T>> for Standard
where
    T: FloatComponent,
    Standard: Distribution<T>,
{
    // `a` and `b` both range from (-128.0, 127.0)
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Lab<Wp, T> {
        Lab {
            l: rng.gen() * from_f64(100.0),
            a: rng.gen() * from_f64(255.0) - from_f64(128.0),
            b: rng.gen() * from_f64(255.0) - from_f64(128.0),
            white_point: PhantomData,
        }
    }
}

#[cfg(feature = "random")]
pub struct UniformLab<Wp, T>
where
    T: FloatComponent + SampleUniform,
{
    l: Uniform<T>,
    a: Uniform<T>,
    b: Uniform<T>,
    white_point: PhantomData<Wp>,
}

#[cfg(feature = "random")]
impl<Wp, T> SampleUniform for Lab<Wp, T>
where
    T: FloatComponent + SampleUniform,
{
    type Sampler = UniformLab<Wp, T>;
}

#[cfg(feature = "random")]
impl<Wp, T> UniformSampler for UniformLab<Wp, T>
where
    T: FloatComponent + SampleUniform,
{
    type X = Lab<Wp, T>;

    fn new<B1, B2>(low_b: B1, high_b: B2) -> Self
    where
        B1: SampleBorrow<Self::X> + Sized,
        B2: SampleBorrow<Self::X> + Sized,
    {
        let low = *low_b.borrow();
        let high = *high_b.borrow();

        UniformLab {
            l: Uniform::new::<_, T>(low.l, high.l),
            a: Uniform::new::<_, T>(low.a, high.a),
            b: Uniform::new::<_, T>(low.b, high.b),
            white_point: PhantomData,
        }
    }

    fn new_inclusive<B1, B2>(low_b: B1, high_b: B2) -> Self
    where
        B1: SampleBorrow<Self::X> + Sized,
        B2: SampleBorrow<Self::X> + Sized,
    {
        let low = *low_b.borrow();
        let high = *high_b.borrow();

        UniformLab {
            l: Uniform::new_inclusive::<_, T>(low.l, high.l),
            a: Uniform::new_inclusive::<_, T>(low.a, high.a),
            b: Uniform::new_inclusive::<_, T>(low.b, high.b),
            white_point: PhantomData,
        }
    }

    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Lab<Wp, T> {
        Lab {
            l: self.l.sample(rng),
            a: self.a.sample(rng),
            b: self.b.sample(rng),
            white_point: PhantomData,
        }
    }
}

#[cfg(feature = "bytemuck")]
unsafe impl<Wp, T> bytemuck::Zeroable for Lab<Wp, T> where T: bytemuck::Zeroable {}

#[cfg(feature = "bytemuck")]
unsafe impl<Wp: 'static, T> bytemuck::Pod for Lab<Wp, T> where T: bytemuck::Pod {}

#[cfg(test)]
mod test {
    use super::Lab;
    use crate::white_point::D65;
    use crate::{FromColor, LinSrgb};

    #[test]
    fn red() {
        let a = Lab::from_color(LinSrgb::new(1.0, 0.0, 0.0));
        let b = Lab::new(53.23288, 80.09246, 67.2031);
        assert_relative_eq!(a, b, epsilon = 0.01);
    }

    #[test]
    fn green() {
        let a = Lab::from_color(LinSrgb::new(0.0, 1.0, 0.0));
        let b = Lab::new(87.73704, -86.184654, 83.18117);
        assert_relative_eq!(a, b, epsilon = 0.01);
    }

    #[test]
    fn blue() {
        let a = Lab::from_color(LinSrgb::new(0.0, 0.0, 1.0));
        let b = Lab::new(32.302586, 79.19668, -107.863686);
        assert_relative_eq!(a, b, epsilon = 0.01);
    }

    #[test]
    fn ranges() {
        assert_ranges! {
            Lab<D65, f64>;
            clamped {
                l: 0.0 => 100.0,
                a: -128.0 => 127.0,
                b: -128.0 => 127.0
            }
            clamped_min {}
            unclamped {}
        }
    }

    raw_pixel_conversion_tests!(Lab<D65>: l, a, b);
    raw_pixel_conversion_fail_tests!(Lab<D65>: l, a, b);

    #[test]
    fn check_min_max_components() {
        assert_relative_eq!(Lab::<D65, f32>::min_l(), 0.0);
        assert_relative_eq!(Lab::<D65, f32>::min_a(), -128.0);
        assert_relative_eq!(Lab::<D65, f32>::min_b(), -128.0);
        assert_relative_eq!(Lab::<D65, f32>::max_l(), 100.0);
        assert_relative_eq!(Lab::<D65, f32>::max_a(), 127.0);
        assert_relative_eq!(Lab::<D65, f32>::max_b(), 127.0);
    }

    #[cfg(feature = "serializing")]
    #[test]
    fn serialize() {
        let serialized = ::serde_json::to_string(&Lab::<D65>::new(0.3, 0.8, 0.1)).unwrap();

        assert_eq!(serialized, r#"{"l":0.3,"a":0.8,"b":0.1}"#);
    }

    #[cfg(feature = "serializing")]
    #[test]
    fn deserialize() {
        let deserialized: Lab = ::serde_json::from_str(r#"{"l":0.3,"a":0.8,"b":0.1}"#).unwrap();

        assert_eq!(deserialized, Lab::new(0.3, 0.8, 0.1));
    }

    #[cfg(feature = "random")]
    test_uniform_distribution! {
        Lab<D65, f32> {
            l: (0.0, 100.0),
            a: (-128.0, 127.0),
            b: (-128.0, 127.0)
        },
        min: Lab::new(0.0f32, -128.0, -128.0),
        max: Lab::new(100.0, 127.0, 127.0)
    }
}
