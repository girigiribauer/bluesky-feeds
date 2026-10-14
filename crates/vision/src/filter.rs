use crate::img::Image;

#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            data: vec![0.0; w * h],
        }
    }

    pub fn from_u8(w: usize, h: usize, data: &[u8]) -> Self {
        Self {
            w,
            h,
            data: data.iter().map(|&v| v as f32).collect(),
        }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.w + x]
    }
}

pub fn reflect101(i: isize, len: usize) -> usize {
    if len == 1 {
        return 0;
    }
    let n = len as isize;
    let mut i = i;
    loop {
        if i < 0 {
            i = -i;
        } else if i >= n {
            i = 2 * n - i - 2;
        } else {
            return i as usize;
        }
    }
}

pub fn box_mean(src: &Plane, kw: usize, kh: usize) -> Plane {
    let (w, h) = (src.w, src.h);
    let (ax, ay) = ((kw / 2) as isize, (kh / 2) as isize);
    let mut rows = vec![0f64; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0f64;
            for k in 0..kw as isize {
                s += src.at(reflect101(x as isize - ax + k, w), y) as f64;
            }
            rows[y * w + x] = s;
        }
    }
    let scale = 1.0 / (kw * kh) as f64;
    let mut out = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut s = 0f64;
            for k in 0..kh as isize {
                s += rows[reflect101(y as isize - ay + k, h) * w + x];
            }
            out.data[y * w + x] = (s * scale) as f32;
        }
    }
    out
}

pub fn laplacian(src: &Plane) -> Plane {
    let (w, h) = (src.w, src.h);
    let mut out = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let at = |dx: isize, dy: isize| {
                src.at(
                    reflect101(x as isize + dx, w),
                    reflect101(y as isize + dy, h),
                ) as f64
            };
            let v = at(0, -1) + at(-1, 0) - 4.0 * at(0, 0) + at(1, 0) + at(0, 1);
            out.data[y * w + x] = v as f32;
        }
    }
    out
}

const EXP_TAB: [u64; 64] = [
    0x3ff0000000000000,
    0x3ff02c9a3e778061,
    0x3ff059b0d3158574,
    0x3ff0874518759bc8,
    0x3ff0b5586cf9890f,
    0x3ff0e3ec32d3d1a2,
    0x3ff11301d0125b51,
    0x3ff1429aaea92de0,
    0x3ff172b83c7d517b,
    0x3ff1a35beb6fcb75,
    0x3ff1d4873168b9aa,
    0x3ff2063b88628cd6,
    0x3ff2387a6e756238,
    0x3ff26b4565e27cdd,
    0x3ff29e9df51fdee1,
    0x3ff2d285a6e4030b,
    0x3ff306fe0a31b715,
    0x3ff33c08b26416ff,
    0x3ff371a7373aa9cb,
    0x3ff3a7db34e59ff7,
    0x3ff3dea64c123422,
    0x3ff4160a21f72e2a,
    0x3ff44e086061892d,
    0x3ff486a2b5c13cd0,
    0x3ff4bfdad5362a27,
    0x3ff4f9b2769d2ca7,
    0x3ff5342b569d4f82,
    0x3ff56f4736b527da,
    0x3ff5ab07dd485429,
    0x3ff5e76f15ad2148,
    0x3ff6247eb03a5585,
    0x3ff6623882552225,
    0x3ff6a09e667f3bcd,
    0x3ff6dfb23c651a2f,
    0x3ff71f75e8ec5f74,
    0x3ff75feb564267c9,
    0x3ff7a11473eb0187,
    0x3ff7e2f336cf4e62,
    0x3ff82589994cce13,
    0x3ff868d99b4492ed,
    0x3ff8ace5422aa0db,
    0x3ff8f1ae99157736,
    0x3ff93737b0cdc5e5,
    0x3ff97d829fde4e50,
    0x3ff9c49182a3f090,
    0x3ffa0c667b5de565,
    0x3ffa5503b23e255d,
    0x3ffa9e6b5579fdbf,
    0x3ffae89f995ad3ad,
    0x3ffb33a2b84f15fb,
    0x3ffb7f76f2fb5e47,
    0x3ffbcc1e904bc1d2,
    0x3ffc199bdd85529c,
    0x3ffc67f12e57d14b,
    0x3ffcb720dcef9069,
    0x3ffd072d4a07897c,
    0x3ffd5818dcfba487,
    0x3ffda9e603db3285,
    0x3ffdfc97337b9b5f,
    0x3ffe502ee78b3ff6,
    0x3ffea4afa2a490da,
    0x3ffefa1bee615a27,
    0x3fff50765b6e4540,
    0x3fffa7c1819e90d8,
];

pub fn soft_exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return if x > 0.0 { x } else { 0.0 };
    }
    let poly = f64::from_bits(0x3f83ce0f3e46f431);
    let a5 = 1.0 / poly;
    let a4 = f64::from_bits(0x3fe62e42fefa39f1) / poly;
    let a3 = f64::from_bits(0x3fcebfbdff82a45a) / poly;
    let a2 = f64::from_bits(0x3fac6b08d81fec75) / poly;
    let a1 = f64::from_bits(0x3f83b2a72b4f3cd3) / poly;
    let a0 = f64::from_bits(0x3f55e7aa1566c2a4) / poly;
    let prescale = f64::from_bits(0x3ff71547652b82fe) * 64.0;
    let max_val = 3000.0 * 64.0;
    let mut x0 = if (x.to_bits() >> 52) & 0x7ff > 1023 + 10 {
        if x < 0.0 {
            -max_val
        } else {
            max_val
        }
    } else {
        x * prescale
    };
    let val0 = x0.round_ties_even() as i32;
    let t = ((val0 >> 6) + 1023).clamp(0, 2047);
    let buf = f64::from_bits((t as u64) << 52);
    x0 = (x0 - x0.round_ties_even()) * (1.0 / 64.0);
    buf * poly
        * f64::from_bits(EXP_TAB[(val0 & 63) as usize])
        * (((((a0 * x0 + a1) * x0 + a2) * x0 + a3) * x0 + a4) * x0 + a5)
}

const LOG_TAB: [u64; 512] = [
    0x0000000000000000,
    0x3ff0000000000000,
    0x3f6ff00aa2b10bc0,
    0x3fefe01fe01fe020,
    0x3f7fe02a6b106788,
    0x3fefc07f01fc07f0,
    0x3f87dc475f810a76,
    0x3fefa11caa01fa12,
    0x3f8fc0a8b0fc03e3,
    0x3fef81f81f81f820,
    0x3f93cea44346a574,
    0x3fef6310aca0dbb5,
    0x3f97b91b07d5b11a,
    0x3fef44659e4a4271,
    0x3f9b9fc027af9197,
    0x3fef25f644230ab5,
    0x3f9f829b0e783300,
    0x3fef07c1f07c1f08,
    0x3fa1b0d98923d97f,
    0x3feee9c7f8458e02,
    0x3fa39e87b9febd5f,
    0x3feecc07b301ecc0,
    0x3fa58a5bafc8e4d4,
    0x3feeae807aba01eb,
    0x3fa77458f632dcfc,
    0x3fee9131abf0b767,
    0x3fa95c830ec8e3eb,
    0x3fee741aa59750e4,
    0x3fab42dd711971be,
    0x3fee573ac901e574,
    0x3fad276b8adb0b52,
    0x3fee3a9179dc1a73,
    0x3faf0a30c01162a6,
    0x3fee1e1e1e1e1e1e,
    0x3fb075983598e471,
    0x3fee01e01e01e01e,
    0x3fb16536eea37ae0,
    0x3fede5d6e3f8868a,
    0x3fb253f62f0a1416,
    0x3fedca01dca01dca,
    0x3fb341d7961bd1d0,
    0x3fedae6076b981db,
    0x3fb42edcbea646f0,
    0x3fed92f2231e7f8a,
    0x3fb51b073f06183f,
    0x3fed77b654b82c34,
    0x3fb60658a93750c3,
    0x3fed5cac807572b2,
    0x3fb6f0d28ae56b4b,
    0x3fed41d41d41d41d,
    0x3fb7da766d7b12cc,
    0x3fed272ca3fc5b1a,
    0x3fb8c345d6319b20,
    0x3fed0cb58f6ec074,
    0x3fb9ab42462033ac,
    0x3fecf26e5c44bfc6,
    0x3fba926d3a4ad563,
    0x3fecd85689039b0b,
    0x3fbb78c82bb0eda1,
    0x3fecbe6d9601cbe7,
    0x3fbc5e548f5bc743,
    0x3feca4b3055ee191,
    0x3fbd4313d66cb35d,
    0x3fec8b265afb8a42,
    0x3fbe27076e2af2e5,
    0x3fec71c71c71c71c,
    0x3fbf0a30c01162a6,
    0x3fec5894d10d4986,
    0x3fbfec9131dbeaba,
    0x3fec3f8f01c3f8f0,
    0x3fc0671512ca596e,
    0x3fec26b5392ea01c,
    0x3fc0d77e7cd08e59,
    0x3fec0e070381c0e0,
    0x3fc14785846742ac,
    0x3febf583ee868d8b,
    0x3fc1b72ad52f67a0,
    0x3febdd2b899406f7,
    0x3fc2266f190a5acb,
    0x3febc4fd65883e7b,
    0x3fc29552f81ff523,
    0x3febacf914c1bad0,
    0x3fc303d718e47fd2,
    0x3feb951e2b18ff23,
    0x3fc371fc201e8f74,
    0x3feb7d6c3dda338b,
    0x3fc3dfc2b0ecc629,
    0x3feb65e2e3beee05,
    0x3fc44d2b6ccb7d1e,
    0x3feb4e81b4e81b4f,
    0x3fc4ba36f39a55e5,
    0x3feb37484ad806ce,
    0x3fc526e5e3a1b437,
    0x3feb2036406c80d9,
    0x3fc59338d9982085,
    0x3feb094b31d922a4,
    0x3fc5ff3070a793d3,
    0x3feaf286bca1af28,
    0x3fc66acd4272ad50,
    0x3feadbe87f94905e,
    0x3fc6d60fe719d21c,
    0x3feac5701ac5701b,
    0x3fc740f8f54037a4,
    0x3feaaf1d2f87ebfd,
    0x3fc7ab890210d909,
    0x3fea98ef606a63be,
    0x3fc815c0a14357ea,
    0x3fea82e65130e159,
    0x3fc87fa06520c910,
    0x3fea6d01a6d01a6d,
    0x3fc8e928de886d40,
    0x3fea574107688a4a,
    0x3fc9525a9cf456b4,
    0x3fea41a41a41a41a,
    0x3fc9bb362e7dfb83,
    0x3fea2c2a87c51ca0,
    0x3fca23bc1fe2b563,
    0x3fea16d3f97a4b02,
    0x3fca8becfc882f18,
    0x3fea01a01a01a01a,
    0x3fcaf3c94e80bff2,
    0x3fe9ec8e951033d9,
    0x3fcb5b519e8fb5a4,
    0x3fe9d79f176b682d,
    0x3fcbc286742d8cd6,
    0x3fe9c2d14ee4a102,
    0x3fcc2968558c18c0,
    0x3fe9ae24ea5510da,
    0x3fcc8ff7c79a9a21,
    0x3fe999999999999a,
    0x3fccf6354e09c5dc,
    0x3fe9852f0d8ec0ff,
    0x3fcd5c216b4fbb91,
    0x3fe970e4f80cb872,
    0x3fcdc1bca0abec7d,
    0x3fe95cbb0be377ae,
    0x3fce27076e2af2e5,
    0x3fe948b0fcd6e9e0,
    0x3fce8c0252aa5a5f,
    0x3fe934c67f9b2ce6,
    0x3fcef0adcbdc5936,
    0x3fe920fb49d0e229,
    0x3fcf550a564b7b37,
    0x3fe90d4f120190d5,
    0x3fcfb9186d5e3e2a,
    0x3fe8f9c18f9c18fa,
    0x3fd00e6c45ad501c,
    0x3fe8e6527af1373f,
    0x3fd0402594b4d040,
    0x3fe8d3018d3018d3,
    0x3fd071b85fcd590d,
    0x3fe8bfce8062ff3a,
    0x3fd0a324e27390e3,
    0x3fe8acb90f6bf3aa,
    0x3fd0d46b579ab74b,
    0x3fe899c0f601899c,
    0x3fd1058bf9ae4ad5,
    0x3fe886e5f0abb04a,
    0x3fd136870293a8b0,
    0x3fe87427bcc092b9,
    0x3fd1675cababa60e,
    0x3fe8618618618618,
    0x3fd1980d2dd4236f,
    0x3fe84f00c2780614,
    0x3fd1c898c16999fa,
    0x3fe83c977ab2bedd,
    0x3fd1f8ff9e48a2f2,
    0x3fe82a4a0182a4a0,
    0x3fd22941fbcf7965,
    0x3fe8181818181818,
    0x3fd2596010df7639,
    0x3fe8060180601806,
    0x3fd2895a13de86a3,
    0x3fe7f405fd017f40,
    0x3fd2b9303ab89d24,
    0x3fe7e225515a4f1d,
    0x3fd2e8e2bae11d30,
    0x3fe7d05f417d05f4,
    0x3fd31871c9544184,
    0x3fe7beb3922e017c,
    0x3fd347dd9a987d54,
    0x3fe7ad2208e0ecc3,
    0x3fd3772662bfd85a,
    0x3fe79baa6bb6398b,
    0x3fd3a64c556945e9,
    0x3fe78a4c8178a4c8,
    0x3fd3d54fa5c1f70f,
    0x3fe77908119ac60d,
    0x3fd404308686a7e3,
    0x3fe767dce434a9b1,
    0x3fd432ef2a04e813,
    0x3fe756cac201756d,
    0x3fd4618bc21c5ec2,
    0x3fe745d1745d1746,
    0x3fd49006804009d0,
    0x3fe734f0c541fe8d,
    0x3fd4be5f957778a0,
    0x3fe724287f46debc,
    0x3fd4ec9732600269,
    0x3fe713786d9c7c09,
    0x3fd51aad872df82d,
    0x3fe702e05c0b8170,
    0x3fd548a2c3add262,
    0x3fe6f26016f26017,
    0x3fd5767717455a6c,
    0x3fe6e1f76b4337c7,
    0x3fd5a42ab0f4cfe1,
    0x3fe6d1a62681c861,
    0x3fd5d1bdbf5809ca,
    0x3fe6c16c16c16c17,
    0x3fd5ff3070a793d3,
    0x3fe6b1490aa31a3d,
    0x3fd62c82f2b9c795,
    0x3fe6a13cd1537290,
    0x3fd659b57303e1f2,
    0x3fe691473a88d0c0,
    0x3fd686c81e9b14ae,
    0x3fe6816816816817,
    0x3fd6b3bb2235943d,
    0x3fe6719f3601671a,
    0x3fd6e08eaa2ba1e3,
    0x3fe661ec6a5122f9,
    0x3fd70d42e2789235,
    0x3fe6524f853b4aa3,
    0x3fd739d7f6bbd006,
    0x3fe642c8590b2164,
    0x3fd7664e1239dbce,
    0x3fe63356b88ac0de,
    0x3fd792a55fdd47a2,
    0x3fe623fa77016240,
    0x3fd7bede0a37afbf,
    0x3fe614b36831ae94,
    0x3fd7eaf83b82afc3,
    0x3fe6058160581606,
    0x3fd816f41da0d495,
    0x3fe5f66434292dfc,
    0x3fd842d1da1e8b17,
    0x3fe5e75bb8d015e7,
    0x3fd86e919a330ba0,
    0x3fe5d867c3ece2a5,
    0x3fd89a3386c1425a,
    0x3fe5c9882b931057,
    0x3fd8c5b7c858b48a,
    0x3fe5babcc647fa91,
    0x3fd8f11e873662c7,
    0x3fe5ac056b015ac0,
    0x3fd91c67eb45a83d,
    0x3fe59d61f123ccaa,
    0x3fd947941c2116fa,
    0x3fe58ed2308158ed,
    0x3fd972a341135158,
    0x3fe5805601580560,
    0x3fd99d958117e08a,
    0x3fe571ed3c506b3a,
    0x3fd9c86b02dc0862,
    0x3fe56397ba7c52e2,
    0x3fd9f323ecbf984b,
    0x3fe5555555555555,
    0x3fda1dc064d5b995,
    0x3fe54725e6bb82fe,
    0x3fda484090e5bb0a,
    0x3fe5390948f40feb,
    0x3fda72a4966bd9ea,
    0x3fe52aff56a8054b,
    0x3fda9cec9a9a0849,
    0x3fe51d07eae2f815,
    0x3fdac718c258b0e4,
    0x3fe50f22e111c4c5,
    0x3fdaf1293247786b,
    0x3fe5015015015015,
    0x3fdb1b1e0ebdfc5b,
    0x3fe4f38f62dd4c9b,
    0x3fdb44f77bcc8f62,
    0x3fe4e5e0a72f0539,
    0x3fdb6eb59d3cf35d,
    0x3fe4d843bedc2c4c,
    0x3fdb9858969310fb,
    0x3fe4cab88725af6e,
    0x3fdbc1e08b0dad0a,
    0x3fe4bd3edda68fe1,
    0x3fdbeb4d9da71b7b,
    0x3fe4afd6a052bf5b,
    0x3fdc149ff115f026,
    0x3fe4a27fad76014a,
    0x3fdc3dd7a7cdad4d,
    0x3fe49539e3b2d067,
    0x3fdc66f4e3ff6ff7,
    0x3fe4880522014880,
    0x3fdc8ff7c79a9a21,
    0x3fe47ae147ae147b,
    0x3fdcb8e0744d7ac9,
    0x3fe46dce34596066,
    0x3fdce1af0b85f3eb,
    0x3fe460cbc7f5cf9a,
    0x3fdd0a63ae721e64,
    0x3fe453d9e2c776ca,
    0x3fdd32fe7e00ebd5,
    0x3fe446f86562d9fb,
    0x3fdd5b7f9ae2c683,
    0x3fe43a2730abee4d,
    0x3fdd83e7258a2f3e,
    0x3fe42d6625d51f87,
    0x3fddac353e2c5954,
    0x3fe420b5265e5951,
    0x3fddd46a04c1c4a0,
    0x3fe4141414141414,
    0x3fddfc859906d5b5,
    0x3fe40782d10e6566,
    0x3fde24881a7c6c26,
    0x3fe3fb013fb013fb,
    0x3fde4c71a8687704,
    0x3fe3ee8f42a5af07,
    0x3fde744261d68787,
    0x3fe3e22cbce4a902,
    0x3fde9bfa659861f5,
    0x3fe3d5d991aa75c6,
    0x3fdec399d2468cc0,
    0x3fe3c995a47babe7,
    0x3fdeeb20c640ddf4,
    0x3fe3bd60d9232955,
    0x3fdf128f5faf06ec,
    0x3fe3b13b13b13b14,
    0x3fdf39e5bc811e5b,
    0x3fe3a524387ac822,
    0x3fdf6123fa7028ac,
    0x3fe3991c2c187f63,
    0x3fdf884a36fe9ec2,
    0x3fe38d22d366088e,
    0x3fdfaf588f78f31e,
    0x3fe3813813813814,
    0x3fdfd64f20f61571,
    0x3fe3755bd1c945ee,
    0x3fdffd2e0857f498,
    0x3fe3698df3de0748,
    0x3fe011fab125ff8a,
    0x3fe35dce5f9f2af8,
    0x3fe02552a5a5d0fe,
    0x3fe3521cfb2b78c1,
    0x3fe0389eefce633b,
    0x3fe34679ace01346,
    0x3fe04bdf9da926d2,
    0x3fe33ae45b57bcb2,
    0x3fe05f14bd26459c,
    0x3fe32f5ced6a1dfa,
    0x3fe0723e5c1cdf40,
    0x3fe323e34a2b10bf,
    0x3fe0855c884b450e,
    0x3fe3187758e9ebb6,
    0x3fe0986f4f573520,
    0x3fe30d190130d190,
    0x3fe0ab76bece14d1,
    0x3fe301c82ac40260,
    0x3fe0be72e4252a82,
    0x3fe2f684bda12f68,
    0x3fe0d163ccb9d6b7,
    0x3fe2eb4ea1fed14b,
    0x3fe0e44985d1cc8b,
    0x3fe2e025c04b8097,
    0x3fe0f7241c9b497d,
    0x3fe2d50a012d50a0,
    0x3fe109f39e2d4c96,
    0x3fe2c9fb4d812ca0,
    0x3fe11cb81787ccf8,
    0x3fe2bef98e5a3711,
    0x3fe12f719593efbc,
    0x3fe2b404ad012b40,
    0x3fe1422025243d44,
    0x3fe2a91c92f3c105,
    0x3fe154c3d2f4d5e9,
    0x3fe29e4129e4129e,
    0x3fe1675cababa60e,
    0x3fe293725bb804a5,
    0x3fe179eabbd899a0,
    0x3fe288b01288b013,
    0x3fe18c6e0ff5cf06,
    0x3fe27dfa38a1ce4d,
    0x3fe19ee6b467c96e,
    0x3fe27350b8812735,
    0x3fe1b154b57da29e,
    0x3fe268b37cd60127,
    0x3fe1c3b81f713c24,
    0x3fe25e22708092f1,
    0x3fe1d610fe677003,
    0x3fe2539d7e9177b2,
    0x3fe1e85f5e7040d0,
    0x3fe2492492492492,
    0x3fe1faa34b87094c,
    0x3fe23eb79717605b,
    0x3fe20cdcd192ab6d,
    0x3fe23456789abcdf,
    0x3fe21f0bfc65beeb,
    0x3fe22a0122a0122a,
    0x3fe23130d7bebf42,
    0x3fe21fb78121fb78,
    0x3fe2434b6f483933,
    0x3fe21579804855e6,
    0x3fe2555bce98f7cb,
    0x3fe20b470c67c0d9,
    0x3fe26762013430df,
    0x3fe2012012012012,
    0x3fe2795e1289b11a,
    0x3fe1f7047dc11f70,
    0x3fe28b500df60782,
    0x3fe1ecf43c7fb84c,
    0x3fe29d37fec2b08a,
    0x3fe1e2ef3b3fb874,
    0x3fe2af15f02640ad,
    0x3fe1d8f5672e4abd,
    0x3fe2c0e9ed448e8b,
    0x3fe1cf06ada2811d,
    0x3fe2d2b4012edc9d,
    0x3fe1c522fc1ce059,
    0x3fe2e47436e40268,
    0x3fe1bb4a4046ed29,
    0x3fe2f62a99509546,
    0x3fe1b17c67f2bae3,
    0x3fe307d7334f10be,
    0x3fe1a7b9611a7b96,
    0x3fe3197a0fa7fe6a,
    0x3fe19e0119e0119e,
    0x3fe32b1339121d71,
    0x3fe19453808ca29c,
    0x3fe33ca2ba328994,
    0x3fe18ab083902bdb,
    0x3fe34e289d9ce1d3,
    0x3fe1811811811812,
    0x3fe35fa4edd36ea0,
    0x3fe1778a191bd684,
    0x3fe37117b54747b5,
    0x3fe16e0689427379,
    0x3fe38280fe58797e,
    0x3fe1648d50fc3201,
    0x3fe393e0d3562a19,
    0x3fe15b1e5f75270d,
    0x3fe3a5373e7ebdf9,
    0x3fe151b9a3fdd5c9,
    0x3fe3b68449fffc22,
    0x3fe1485f0e0acd3b,
    0x3fe3c7c7fff73205,
    0x3fe13f0e8d344724,
    0x3fe3d9026a7156fa,
    0x3fe135c81135c811,
    0x3fe3ea33936b2f5b,
    0x3fe12c8b89edc0ac,
    0x3fe3fb5b84d16f42,
    0x3fe12358e75d3033,
    0x3fe40c7a4880dce9,
    0x3fe11a3019a74826,
    0x3fe41d8fe84672ae,
    0x3fe1111111111111,
    0x3fe42e9c6ddf80bf,
    0x3fe107fbbe011080,
    0x3fe43f9fe2f9ce67,
    0x3fe0fef010fef011,
    0x3fe4509a5133bb0a,
    0x3fe0f5edfab325a2,
    0x3fe4618bc21c5ec2,
    0x3fe0ecf56be69c90,
    0x3fe472743f33aaad,
    0x3fe0e40655826011,
    0x3fe48353d1ea88df,
    0x3fe0db20a88f4696,
    0x3fe4942a83a2fc07,
    0x3fe0d24456359e3a,
    0x3fe4a4f85db03ebb,
    0x3fe0c9714fbcda3b,
    0x3fe4b5bd6956e273,
    0x3fe0c0a7868b4171,
    0x3fe4c679afccee39,
    0x3fe0b7e6ec259dc8,
    0x3fe4d72d3a39fd00,
    0x3fe0af2f722eecb5,
    0x3fe4e7d811b75bb0,
    0x3fe0a6810a6810a7,
    0x3fe4f87a3f5026e8,
    0x3fe09ddba6af8360,
    0x3fe50913cc01686b,
    0x3fe0953f39010954,
    0x3fe519a4c0ba3446,
    0x3fe08cabb37565e2,
    0x3fe52a2d265bc5aa,
    0x3fe0842108421084,
    0x3fe53aad05b99b7c,
    0x3fe07b9f29b8eae2,
    0x3fe54b2467999497,
    0x3fe073260a47f7c6,
    0x3fe55b9354b40bcd,
    0x3fe06ab59c7912fb,
    0x3fe56bf9d5b3f399,
    0x3fe0624dd2f1a9fc,
    0x3fe57c57f336f190,
    0x3fe059eea0727586,
    0x3fe58cadb5cd7989,
    0x3fe05197f7d73404,
    0x3fe59cfb25fae87d,
    0x3fe04949cc1664c5,
    0x3fe5ad404c359f2c,
    0x3fe0410410410410,
    0x3fe5bd7d30e71c73,
    0x3fe038c6b78247fc,
    0x3fe5cdb1dc6c1764,
    0x3fe03091b51f5e1a,
    0x3fe5ddde57149923,
    0x3fe02864fc7729e9,
    0x3fe5ee02a9241675,
    0x3fe0204081020408,
    0x3fe5fe1edad18918,
    0x3fe0182436517a37,
    0x3fe60e32f44788d8,
    0x3fe0101010101010,
    0x3fe62e42fefa39ef,
    0x3fe0000000000000,
];
pub fn soft_log(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    let a7 = 1.0f64;
    let a6 = -1.0 / 2.0;
    let a5 = 1.0 / 3.0;
    let a4 = -1.0 / 4.0;
    let a3 = 1.0 / 5.0;
    let a2 = -1.0 / 6.0;
    let a1 = 1.0 / 7.0;
    let a0 = -1.0 / 8.0;
    let bits = x.to_bits();
    let h0 = ((bits >> (52 - 8)) & 0xff) as usize;
    let buf = f64::from_bits((1023u64 << 52) + (bits & ((1u64 << (52 - 8)) - 1))) - 1.0;
    let tab0 = f64::from_bits(LOG_TAB[2 * h0]);
    let tab1 = f64::from_bits(LOG_TAB[2 * h0 + 1]);
    let mut x0 = buf * tab1;
    if h0 == 255 {
        x0 += -1.0 / 512.0;
    }
    let xq = x0 * x0;
    let ln2 = f64::from_bits(0x3fe62e42fefa39ef);
    let e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    ln2 * e as f64
        + tab0
        + (((a0 * xq + a2) * xq + a4) * xq + a6) * xq
        + (((a1 * xq + a3) * xq + a5) * xq + a7) * x0
}

pub fn soft_pow(x: f64, y: f64) -> f64 {
    assert!(x > 0.0 && y != y.round_ties_even());
    soft_exp(y * soft_log(x))
}

pub fn soft_cbrt(x: f32) -> f32 {
    if x.is_nan() {
        return f32::NAN;
    }
    if x.is_infinite() {
        return x;
    }
    let v = x.to_bits();
    let s = v >> 31;
    let mut ex = ((v >> 23) & 0xff) as i32 - 127;
    let mut shx = ex % 3;
    if shx >= 0 {
        shx -= 3;
    }
    ex = (ex - shx) / 3 - 1;
    let fr = f64::from_bits((((shx + 1023) as u64) << 52) + (((v & 0x7fffff) as u64) << 29));
    let a = [
        0x4046a09e6653ba70u64,
        0x406808f46c6116e0,
        0x405dca97439cae14,
        0x402add70d2827500,
        0x3fc4f15f83f55d2d,
        0x402d9e20660edb21,
        0x4062ff15c0285815,
        0x406510d06a8112ce,
        0x4040fecbc9e2c375,
    ]
    .map(f64::from_bits);
    let fr = ((((a[0] * fr + a[1]) * fr + a[2]) * fr + a[3]) * fr + a[4])
        / ((((a[5] * fr + a[6]) * fr + a[7]) * fr + a[8]) * fr + 1.0);
    if v & 0x7fff_ffff == 0 {
        return 0.0;
    }
    f32::from_bits(
        (s << 31)
            + (((ex + 127) as u32) << 23)
            + ((fr.to_bits() & 0xf_ffff_ffff_ffff) >> 29) as u32,
    )
}

pub fn gaussian_size(sigma: f64, for_u8: bool) -> usize {
    ((sigma * if for_u8 { 3.0 } else { 4.0 } * 2.0 + 1.0).round_ties_even() as usize) | 1
}

pub fn gaussian_kernel(n: usize, sigma: f64) -> Vec<f64> {
    let scale2x = -0.125 / (sigma * sigma);
    let half = (n - 1) / 2;
    let mut values = Vec::with_capacity(half);
    let mut sum = 0f64;
    let mut x = 1 - n as i64;
    for _ in 0..half {
        let t = soft_exp((x * x) as f64 * scale2x);
        values.push(t);
        sum += t;
        x += 2;
    }
    sum *= 2.0;
    sum += 1.0;
    let mul = 1.0 / sum;
    let mut out = vec![0f64; n];
    for (i, v) in values.iter().enumerate() {
        out[i] = v * mul;
        out[n - 1 - i] = v * mul;
    }
    out[half] = mul;
    out
}

pub fn gaussian(src: &Plane, sigma: f64) -> Plane {
    let n = gaussian_size(sigma, false);
    let k: Vec<f32> = gaussian_kernel(n, sigma)
        .iter()
        .map(|&v| v as f32)
        .collect();
    let kx = if src.w == 1 { vec![1.0] } else { k.clone() };
    let ky = if src.h == 1 { vec![1.0] } else { k };
    if kx.len() == 1 && ky.len() == 1 {
        return src.clone();
    }
    let (w, h) = (src.w, src.h);
    let ax = (kx.len() / 2) as isize;
    let mut rows = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let at = |k: usize| src.at(reflect101(x as isize - ax + k as isize, w), y);
            let mut s = kx[0] * at(0);
            for (i, &f) in kx.iter().enumerate().skip(1) {
                s += f * at(i);
            }
            rows.data[y * w + x] = s;
        }
    }
    let half = ky.len() / 2;
    let mut out = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let at = |dy: isize| rows.at(x, reflect101(y as isize + dy, h));
            let mut s = ky[half] * at(0) + 0.0;
            for j in 1..=half {
                s += ky[half + j] * (at(j as isize) + at(-(j as isize)));
            }
            out.data[y * w + x] = s;
        }
    }
    out
}

const LANES: usize = 4;

fn sobel_rows(src: &Plane, vertical_kernel: bool) -> Plane {
    let (w, h) = (src.w, src.h);
    let vec_end = w / LANES * LANES;
    let mut out = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = src.at(reflect101(x as isize - 1, w), y);
            let c = src.at(x, y);
            let b = src.at(reflect101(x as isize + 1, w), y);
            out.data[y * w + x] = if !vertical_kernel {
                b - a
            } else if x < vec_end {
                ((a + b) + c) + c
            } else if x < w - (w - vec_end) % 2 {
                (a + c * 2.0) + b
            } else {
                2.0 * c + 1.0 * (b + a)
            };
        }
    }
    out
}

fn sobel_cols(rows: &Plane, smooth_kernel: bool) -> Plane {
    let (w, h) = (rows.w, rows.h);
    let vec_end = w / LANES * LANES;
    let mut out = Plane::new(w, h);
    for y in 0..h {
        let up = reflect101(y as isize - 1, h);
        let down = reflect101(y as isize + 1, h);
        for x in 0..w {
            let (s0, s1, s2) = (rows.at(x, up), rows.at(x, y), rows.at(x, down));
            out.data[y * w + x] = if !smooth_kernel {
                (s2 - s0) + 0.0
            } else if x < vec_end {
                (((s0 + s2) + 0.0) + s1) + s1
            } else {
                ((s0 + s1 * 2.0) + s2) + 0.0
            };
        }
    }
    out
}

pub fn sobel_x(src: &Plane) -> Plane {
    sobel_cols(&sobel_rows(src, false), true)
}

pub fn sobel_y(src: &Plane) -> Plane {
    sobel_cols(&sobel_rows(src, true), false)
}

fn rank_rect(src: &Image, kw: usize, kh: usize, take_max: bool) -> Image {
    assert_eq!(src.ch, 1);
    let (w, h) = (src.w, src.h);
    let (ax, ay) = (kw / 2, kh / 2);
    let mut rows = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let lo = x.saturating_sub(ax);
            let hi = (x + kw - ax).min(w);
            let seg = &src.data[y * w + lo..y * w + hi];
            rows[y * w + x] = if take_max {
                *seg.iter().max().unwrap()
            } else {
                *seg.iter().min().unwrap()
            };
        }
    }
    let mut out = Image::new(w, h, 1);
    for y in 0..h {
        let lo = y.saturating_sub(ay);
        let hi = (y + kh - ay).min(h);
        for x in 0..w {
            let it = (lo..hi).map(|r| rows[r * w + x]);
            out.data[y * w + x] = if take_max {
                it.max().unwrap()
            } else {
                it.min().unwrap()
            };
        }
    }
    out
}

pub fn dilate_rect(src: &Image, kw: usize, kh: usize) -> Image {
    rank_rect(src, kw, kh, true)
}

pub fn erode_rect(src: &Image, kw: usize, kh: usize) -> Image {
    rank_rect(src, kw, kh, false)
}

pub fn morph_gradient(src: &Image, kw: usize, kh: usize) -> Image {
    let (d, e) = (dilate_rect(src, kw, kh), erode_rect(src, kw, kh));
    Image {
        data: d
            .data
            .iter()
            .zip(&e.data)
            .map(|(a, b)| a.saturating_sub(*b))
            .collect(),
        ..d
    }
}

pub fn close_rect(src: &Image, kw: usize, kh: usize) -> Image {
    erode_rect(&dilate_rect(src, kw, kh), kw, kh)
}

pub fn otsu(src: &Image) -> f64 {
    let mut hist = [0i64; 256];
    for &v in &src.data {
        hist[v as usize] += 1;
    }
    let scale = 1.0 / src.data.len() as f64;
    let mut mu = 0f64;
    for (i, &c) in hist.iter().enumerate() {
        mu += i as f64 * c as f64;
    }
    mu *= scale;
    let (mut mu1, mut q1, mut max_sigma, mut max_val) = (0f64, 0f64, 0f64, 0f64);
    let eps = f32::EPSILON as f64;
    for (i, &c) in hist.iter().enumerate() {
        let p = c as f64 * scale;
        mu1 *= q1;
        q1 += p;
        let q2 = 1.0 - q1;
        if q1.min(q2) < eps || q1.max(q2) > 1.0 - eps {
            continue;
        }
        mu1 = (mu1 + i as f64 * p) / q1;
        let mu2 = (mu - q1 * mu1) / q2;
        let sigma = q1 * q2 * (mu1 - mu2) * (mu1 - mu2);
        if sigma > max_sigma {
            max_sigma = sigma;
            max_val = i as f64;
        }
    }
    max_val
}

pub fn threshold_binary(src: &Image, thresh: f64) -> Image {
    let t = thresh.floor();
    Image {
        data: src
            .data
            .iter()
            .map(|&v| if v as f64 > t { 255 } else { 0 })
            .collect(),
        ..src.clone()
    }
}

pub fn resize_area(src: &Plane, dw: usize, dh: usize) -> Plane {
    let inv_x = dw as f64 / src.w as f64;
    let inv_y = dh as f64 / src.h as f64;
    let (scale_x, scale_y) = (1.0 / inv_x, 1.0 / inv_y);
    if dw == src.w && dh == src.h {
        return src.clone();
    }
    let (ix, iy) = (scale_x.round_ties_even(), scale_y.round_ties_even());
    let fast = (scale_x - ix).abs() < f64::EPSILON && (scale_y - iy).abs() < f64::EPSILON;
    let mut out = Plane::new(dw, dh);
    if fast && scale_x >= 1.0 && scale_y >= 1.0 {
        let (sx, sy) = (ix as usize, iy as usize);
        let scale = 1.0f32 / (sx * sy) as f32;
        for y in 0..dh {
            for x in 0..dw {
                let mut sum = 0f32;
                for ky in 0..sy {
                    for kx in 0..sx {
                        sum += src.at(x * sx + kx, y * sy + ky);
                    }
                }
                out.data[y * dw + x] = sum * scale;
            }
        }
        return out;
    }
    if scale_x >= 1.0 && scale_y >= 1.0 {
        let xtab = crate::img::area_tab(src.w, dw, 1, scale_x);
        let ytab = crate::img::area_tab(src.h, dh, 1, scale_y);
        let mut sum = vec![0f32; dw];
        let mut buf = vec![0f32; dw];
        let mut prev = ytab[0].di;
        for t in &ytab {
            buf.iter_mut().for_each(|v| *v = 0.0);
            for x in &xtab {
                buf[x.di] += src.at(x.si, t.si) * x.alpha;
            }
            if t.di != prev {
                out.data[prev * dw..(prev + 1) * dw].copy_from_slice(&sum);
                for (v, b) in sum.iter_mut().zip(&buf) {
                    *v = b * t.alpha;
                }
                prev = t.di;
            } else {
                for (v, b) in sum.iter_mut().zip(&buf) {
                    *v += b * t.alpha;
                }
            }
        }
        out.data[prev * dw..(prev + 1) * dw].copy_from_slice(&sum);
        return out;
    }
    for y in 0..dh {
        let fy = ((y as f64 + 0.5) * scale_y - 0.5).max(0.0);
        let (y0, wy) = (fy.floor() as usize, (fy - fy.floor()) as f32);
        let y1 = (y0 + 1).min(src.h - 1);
        for x in 0..dw {
            let fx = ((x as f64 + 0.5) * scale_x - 0.5).max(0.0);
            let (x0, wx) = (fx.floor() as usize, (fx - fx.floor()) as f32);
            let x1 = (x0 + 1).min(src.w - 1);
            let top = src.at(x0.min(src.w - 1), y0.min(src.h - 1)) * (1.0 - wx)
                + src.at(x1, y0.min(src.h - 1)) * wx;
            let bottom = src.at(x0.min(src.w - 1), y1) * (1.0 - wx) + src.at(x1, y1) * wx;
            out.data[y * dw + x] = top * (1.0 - wy) + bottom * wy;
        }
    }
    out
}

fn fixed_kernel(n: usize, sigma: f64) -> Vec<u32> {
    let k = gaussian_kernel(n, sigma);
    let mut out = vec![0u32; n];
    let half = n / 2;
    let (mut err, mut sum) = (0f64, 0i64);
    for i in 0..half {
        let adj = k[i] * 256.0 + err;
        let v = adj.round_ties_even() as i64;
        err = adj - v as f64;
        out[i] = v as u32;
        out[n - 1 - i] = v as u32;
        sum += v;
    }
    out[half] = (256 - sum * 2) as u32;
    out
}

pub fn gaussian_u8(src: &Image, ksize: usize, sigma: f64) -> Image {
    let size = |dim: usize| {
        if dim == 1 {
            1
        } else if ksize == 0 {
            gaussian_size(sigma, true)
        } else {
            ksize
        }
    };
    let (kxn, kyn) = (size(src.w), size(src.h));
    if kxn == 1 && kyn == 1 {
        return src.clone();
    }
    let kx = if kxn == 1 {
        vec![256]
    } else {
        fixed_kernel(kxn, sigma)
    };
    let ky = if kyn == 1 {
        vec![256]
    } else {
        fixed_kernel(kyn, sigma)
    };
    let (w, h, ch) = (src.w, src.h, src.ch);
    let (ax, ay) = ((kx.len() / 2) as isize, (ky.len() / 2) as isize);
    let mut rows = vec![0u32; w * h * ch];
    for y in 0..h {
        for x in 0..w {
            for c in 0..ch {
                let mut s = 0u32;
                for (i, &k) in kx.iter().enumerate() {
                    let sx = reflect101(x as isize - ax + i as isize, w);
                    s += k * src.data[(y * w + sx) * ch + c] as u32;
                }
                rows[(y * w + x) * ch + c] = s;
            }
        }
    }
    let mut out = Image::new(w, h, ch);
    for y in 0..h {
        for x in 0..w {
            for c in 0..ch {
                let mut s = 0u32;
                for (j, &k) in ky.iter().enumerate() {
                    let sy = reflect101(y as isize - ay + j as isize, h);
                    s += k * rows[(sy * w + x) * ch + c];
                }
                out.data[(y * w + x) * ch + c] = ((s + 32768) >> 16).min(255) as u8;
            }
        }
    }
    out
}

pub fn square(src: &Plane) -> Plane {
    Plane {
        w: src.w,
        h: src.h,
        data: src.data.iter().map(|v| v * v).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fnv;

    fn random_int(w: usize, h: usize, max: u32, seed: u64) -> Plane {
        let mut s = seed;
        let data = (0..w * h)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as u32 % (max + 1)) as f32
            })
            .collect();
        Plane { w, h, data }
    }

    const SIZES: [(usize, usize); 7] = [
        (256, 192),
        (109, 256),
        (1, 1),
        (3, 2),
        (17, 5),
        (5, 17),
        (768, 512),
    ];

    fn random_float(w: usize, h: usize, seed: u64) -> Plane {
        let mut s = seed;
        let data = (0..w * h)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (s >> 40) as f32 / 65536.0 * 1.7 - 3.1
            })
            .collect();
        Plane { w, h, data }
    }

    fn random_u8(w: usize, h: usize, levels: u32, seed: u64) -> Image {
        let p = random_int(w, h, levels - 1, seed);
        Image {
            w,
            h,
            ch: 1,
            data: p
                .data
                .iter()
                .map(|&v| (v * (255 / (levels - 1)) as f32) as u8)
                .collect(),
        }
    }

    fn plane_hash(planes: impl IntoIterator<Item = Plane>) -> u64 {
        let mut h = Fnv::new();
        for p in planes {
            h.f32s(&p.data);
        }
        h.0
    }

    fn image_hash(images: impl IntoIterator<Item = Image>) -> u64 {
        let mut h = Fnv::new();
        for i in images {
            h.bytes(&i.data);
        }
        h.0
    }

    /// 箱型ぼかしの出力が、OpenCV と一致することを確かめたときの記録から変わらないこと（大きさ・値の幅・ぼかしの大きさを変えた全63通り）
    #[test]
    fn test_box_mean_stays_recorded() {
        let mut out = Vec::new();
        for (i, &(w, h)) in SIZES.iter().enumerate() {
            for max in [255, 65025, 1020] {
                let p = random_int(w, h, max, i as u64 + max as u64);
                for k in [5, 7, 9] {
                    out.push(box_mean(&p, k, k));
                }
            }
        }
        assert_eq!(plane_hash(out), 12837306975189675302);
    }

    /// ラプラシアンの出力が、OpenCV と一致することを確かめたときの記録から変わらないこと
    #[test]
    fn test_laplacian_stays_recorded() {
        let out = SIZES
            .iter()
            .enumerate()
            .map(|(i, &(w, h))| laplacian(&random_int(w, h, 255, i as u64)));
        assert_eq!(plane_hash(out), 3582540034081471902);
    }

    /// ガウスぼかしの重みが、OpenCV と一致することを確かめたときの記録から変わらないこと（この判定で使う6通りの強さ）
    #[test]
    fn test_gaussian_kernel_stays_recorded() {
        let mut h = Fnv::new();
        for (sigma, for_u8) in [
            (0.7, false),
            (1.4, false),
            (2.8, false),
            (4.0, false),
            (5.6, false),
            (6.0, true),
        ] {
            h.f64s(&gaussian_kernel(gaussian_size(sigma, for_u8), sigma));
        }
        assert_eq!(h.0, 4888611532833002633);
    }

    /// 小数の画像のガウスぼかしの出力が、OpenCV と一致することを確かめたときの記録から変わらないこと（大きさ7通り×強さ5通り×整数と小数の画像）
    #[test]
    fn test_gaussian_stays_recorded() {
        let mut out = Vec::new();
        for (i, &(w, h)) in SIZES.iter().enumerate() {
            for sigma in [0.7, 1.4, 2.8, 4.0, 5.6] {
                for p in [
                    random_int(w, h, 255, i as u64),
                    random_float(w, h, i as u64),
                ] {
                    out.push(gaussian(&p, sigma));
                }
            }
        }
        assert_eq!(plane_hash(out), 4144643315280394731);
    }

    /// ソーベルの出力が、OpenCV と一致することを確かめたときの記録から変わらないこと（横・縦の両方）
    #[test]
    fn test_sobel_stays_recorded() {
        let mut out = Vec::new();
        for (i, &(w, h)) in SIZES.iter().enumerate() {
            for p in [
                gaussian(&random_int(w, h, 255, i as u64), 4.0),
                random_float(w, h, i as u64),
            ] {
                out.push(sobel_x(&p));
                out.push(sobel_y(&p));
            }
        }
        assert_eq!(plane_hash(out), 13163338386070354122);
    }

    /// 膨張・収縮・その差・閉じる処理の出力が、OpenCV と一致することを確かめたときの記録から変わらないこと
    #[test]
    fn test_morphology_stays_recorded() {
        let mut out = Vec::new();
        for (i, &(w, h)) in SIZES.iter().enumerate() {
            for levels in [256, 2] {
                let img = random_u8(w, h, levels, i as u64);
                for (kw, kh) in [(3, 3), (9, 1), (13, 13)] {
                    out.push(erode_rect(&img, kw, kh));
                    out.push(dilate_rect(&img, kw, kh));
                    out.push(morph_gradient(&img, kw, kh));
                    out.push(close_rect(&img, kw, kh));
                }
            }
        }
        assert_eq!(image_hash(out), 9940697225844413077);
    }

    /// 大津の方法の境目と2値化の出力が、OpenCV と一致することを確かめたときの記録から変わらないこと
    #[test]
    fn test_otsu_and_threshold_stay_recorded() {
        let mut h = Fnv::new();
        for (i, &(w, h_)) in SIZES.iter().enumerate() {
            let img = random_u8(w, h_, 256, i as u64 + 50);
            let t = otsu(&img);
            h.f64s(&[t]);
            h.bytes(&threshold_binary(&img, t).data);
            for th in [90.0, 90.5, 0.0, 254.9] {
                h.bytes(&threshold_binary(&img, th).data);
            }
        }
        assert_eq!(h.0, 18041376569461163948);
    }

    /// 小数の画像の面積平均の縮小の出力が、OpenCV と一致することを確かめたときの記録から変わらないこと
    #[test]
    fn test_resize_area_float_stays_recorded() {
        let out = [
            (256, 192),
            (256, 256),
            (192, 256),
            (256, 100),
            (109, 256),
            (128, 128),
            (200, 150),
        ]
        .iter()
        .enumerate()
        .map(|(i, &(w, h))| resize_area(&random_int(w, h, 255, i as u64), 64, 64));
        assert_eq!(plane_hash(out), 1427162088902205059);
    }

    /// 整数の画像のガウスぼかしの出力が、OpenCV と一致することを確かめたときの記録から変わらないこと（白黒・カラー）
    #[test]
    fn test_gaussian_u8_stays_recorded() {
        let mut out = Vec::new();
        for (i, &(w, h)) in SIZES.iter().enumerate() {
            for ch in [1, 3] {
                let p = random_int(w * ch, h, 255, i as u64 + ch as u64);
                let img = Image {
                    w,
                    h,
                    ch,
                    data: p.data.iter().map(|&v| v as u8).collect(),
                };
                for (ksize, sigma) in [(7usize, 0.75), (0, 6.0), (0, 1.0)] {
                    out.push(gaussian_u8(&img, ksize, sigma));
                }
            }
        }
        assert_eq!(image_hash(out), 17804107132311783892);
    }

    fn flat(w: usize, h: usize, v: f32) -> Plane {
        Plane {
            w,
            h,
            data: vec![v; w * h],
        }
    }

    /// 平らな画像は、箱型ぼかし・ガウスぼかし・面積平均の縮小をしても、同じ値のまま平らであること
    #[test]
    fn test_flat_image_stays_flat_when_blurred() {
        let p = flat(40, 30, 117.0);
        assert!(box_mean(&p, 7, 7).data.iter().all(|&v| v == 117.0));
        assert!(gaussian(&p, 2.8)
            .data
            .iter()
            .all(|&v| (v - 117.0).abs() < 1e-3));
        assert!(resize_area(&p, 16, 12).data.iter().all(|&v| v == 117.0));
        let img = Image {
            w: 40,
            h: 30,
            ch: 3,
            data: vec![117; 40 * 30 * 3],
        };
        assert!(gaussian_u8(&img, 0, 6.0).data.iter().all(|&v| v == 117));
    }

    /// 平らな画像は、ラプラシアン・ソーベルが全部 0 になること（明るさの変化が無いため）
    #[test]
    fn test_flat_image_has_no_change() {
        let p = flat(20, 10, 80.0);
        assert!(laplacian(&p).data.iter().all(|&v| v == 0.0));
        assert!(sobel_x(&p).data.iter().all(|&v| v == 0.0));
        assert!(sobel_y(&p).data.iter().all(|&v| v == 0.0));
    }

    /// 横に1ずつ明るくなる画像では、内側のソーベルの横方向が 8（1・2・1 の重みの合計4 × 左右の差2）、縦方向が 0 になること
    #[test]
    fn test_sobel_on_horizontal_ramp() {
        let p = Plane {
            w: 10,
            h: 6,
            data: (0..60).map(|i| (i % 10) as f32).collect(),
        };
        let (gx, gy) = (sobel_x(&p), sobel_y(&p));
        for y in 1..5 {
            for x in 1..9 {
                assert_eq!(gx.data[y * 10 + x], 8.0);
                assert_eq!(gy.data[y * 10 + x], 0.0);
            }
        }
    }

    /// ガウスぼかしの重みは、左右対称で、合計が 1 になること
    #[test]
    fn test_gaussian_kernel_is_symmetric_and_sums_to_one() {
        for sigma in [0.7, 1.4, 2.8, 4.0, 5.6] {
            let k = gaussian_kernel(gaussian_size(sigma, false), sigma);
            assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            assert!(k.iter().zip(k.iter().rev()).all(|(a, b)| a == b));
        }
    }

    /// 1点だけ白い画像を膨張させると四角の大きさに広がり、それを収縮させると元の1点に戻ること
    #[test]
    fn test_dilate_spreads_a_dot_and_erode_restores_it() {
        let mut img = Image {
            w: 15,
            h: 15,
            ch: 1,
            data: vec![0; 225],
        };
        img.data[7 * 15 + 7] = 255;
        let d = dilate_rect(&img, 5, 3);
        for y in 0..15 {
            for x in 0..15 {
                let inside = (5..=9).contains(&x) && (6..=8).contains(&y);
                assert_eq!(
                    d.data[y * 15 + x],
                    if inside { 255 } else { 0 },
                    "({x},{y})"
                );
            }
        }
        assert_eq!(erode_rect(&d, 5, 3), img);
    }

    /// 閉じる処理は、四角より狭いすき間を埋めること
    #[test]
    fn test_close_fills_narrow_gap() {
        let mut img = Image {
            w: 20,
            h: 3,
            ch: 1,
            data: vec![255; 60],
        };
        for y in 0..3 {
            img.data[y * 20 + 10] = 0;
        }
        assert!(close_rect(&img, 3, 1).data.iter().all(|&v| v == 255));
    }

    /// 明るさが2種類だけの画像では、大津の方法の境目が2つの間に入り、2値化で暗い方が 0・明るい方が 255 になること
    #[test]
    fn test_otsu_splits_two_levels() {
        let img = Image {
            w: 10,
            h: 1,
            ch: 1,
            data: vec![50, 50, 50, 50, 200, 200, 200, 200, 200, 200],
        };
        let t = otsu(&img);
        assert!((50.0..200.0).contains(&t), "{t}");
        assert_eq!(
            threshold_binary(&img, t).data,
            vec![0, 0, 0, 0, 255, 255, 255, 255, 255, 255]
        );
    }

    /// 2値化は、境目ちょうどの値を 0 にし、境目より大きい値だけを 255 にすること
    #[test]
    fn test_threshold_keeps_equal_value_dark() {
        let img = Image {
            w: 3,
            h: 1,
            ch: 1,
            data: vec![90, 91, 89],
        };
        assert_eq!(threshold_binary(&img, 90.0).data, vec![0, 255, 0]);
    }

    /// 縦横ちょうど半分への面積平均の縮小は、2×2 の4画素の平均になること
    #[test]
    fn test_resize_area_halves_by_block_average() {
        let p = Plane {
            w: 4,
            h: 2,
            data: vec![0.0, 4.0, 8.0, 8.0, 4.0, 8.0, 0.0, 0.0],
        };
        assert_eq!(resize_area(&p, 2, 1).data, vec![4.0, 4.0]);
    }

    /// 2乗は、それぞれの値を自分自身と掛けた値になること
    #[test]
    fn test_square_multiplies_each_value_by_itself() {
        let p = Plane {
            w: 3,
            h: 1,
            data: vec![-3.0, 0.5, 255.0],
        };
        assert_eq!(square(&p).data, vec![9.0, 0.25, 65025.0]);
    }
}
