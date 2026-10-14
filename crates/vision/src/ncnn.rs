use std::ffi::{c_int, c_void, CString};
use std::path::Path;

use anyhow::Context;

type Ptr = *mut c_void;

pub struct Output {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

extern "C" {
    fn ncnn_option_create() -> Ptr;
    fn ncnn_option_destroy(opt: Ptr);
    fn ncnn_option_set_num_threads(opt: Ptr, n: c_int);
    fn ncnn_option_set_use_local_pool_allocator(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_winograd_convolution(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_sgemm_convolution(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_fp16_packed(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_fp16_storage(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_fp16_arithmetic(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_bf16_packed(opt: Ptr, enabled: c_int);
    fn ncnn_option_set_use_bf16_storage(opt: Ptr, enabled: c_int);
    fn ncnn_net_create() -> Ptr;
    fn ncnn_net_destroy(net: Ptr);
    fn ncnn_net_set_option(net: Ptr, opt: Ptr);
    fn ncnn_net_load_param(net: Ptr, path: *const i8) -> c_int;
    fn ncnn_net_load_model(net: Ptr, path: *const i8) -> c_int;
    fn ncnn_extractor_create(net: Ptr) -> Ptr;
    fn ncnn_extractor_destroy(ex: Ptr);
    fn ncnn_extractor_input(ex: Ptr, name: *const i8, mat: Ptr) -> c_int;
    fn ncnn_extractor_extract(ex: Ptr, name: *const i8, mat: *mut Ptr) -> c_int;
    fn ncnn_mat_create_3d(w: c_int, h: c_int, c: c_int, allocator: Ptr) -> Ptr;
    fn ncnn_mat_destroy(mat: Ptr);
    fn ncnn_mat_get_dims(mat: Ptr) -> c_int;
    fn ncnn_mat_get_w(mat: Ptr) -> c_int;
    fn ncnn_mat_get_h(mat: Ptr) -> c_int;
    fn ncnn_mat_get_c(mat: Ptr) -> c_int;
    fn ncnn_mat_get_elempack(mat: Ptr) -> c_int;
    fn ncnn_mat_get_channel_data(mat: Ptr, c: c_int) -> *mut c_void;
}

pub struct NcnnModel {
    net: Ptr,
    opt: Ptr,
    outputs: usize,
}

impl NcnnModel {
    pub fn load_named(dir: &Path, stem: &str) -> anyhow::Result<Self> {
        Self::load(
            &dir.join(format!("{stem}.ncnn.param")),
            &dir.join(format!("{stem}.ncnn.bin")),
        )
    }

    pub fn load(param: &Path, bin: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(param)
            .with_context(|| format!("failed to read {}", param.display()))?;
        let outputs = output_count(&text);
        anyhow::ensure!(outputs > 0, "no outputs in {}", param.display());
        let p = CString::new(param.to_string_lossy().as_bytes())?;
        let b = CString::new(bin.to_string_lossy().as_bytes())?;
        unsafe {
            let opt = ncnn_option_create();
            ncnn_option_set_num_threads(opt, 1);
            ncnn_option_set_use_local_pool_allocator(opt, 0);
            ncnn_option_set_use_winograd_convolution(opt, 0);
            ncnn_option_set_use_sgemm_convolution(opt, 0);
            ncnn_option_set_use_fp16_packed(opt, 0);
            ncnn_option_set_use_fp16_storage(opt, 0);
            ncnn_option_set_use_fp16_arithmetic(opt, 0);
            ncnn_option_set_use_bf16_packed(opt, 0);
            ncnn_option_set_use_bf16_storage(opt, 0);
            let model = Self {
                net: ncnn_net_create(),
                opt,
                outputs,
            };
            ncnn_net_set_option(model.net, opt);
            anyhow::ensure!(
                ncnn_net_load_param(model.net, p.as_ptr()) == 0,
                "failed to load {}",
                param.display()
            );
            anyhow::ensure!(
                ncnn_net_load_model(model.net, b.as_ptr()) == 0,
                "failed to load {}",
                bin.display()
            );
            Ok(model)
        }
    }

    pub fn run(&self, input: Vec<f32>, shape: [usize; 3]) -> anyhow::Result<Vec<Output>> {
        let [c, h, w] = shape;
        anyhow::ensure!(input.len() == c * h * w, "input size does not match shape");
        let in0 = CString::new("in0")?;
        unsafe {
            let mat = ncnn_mat_create_3d(w as c_int, h as c_int, c as c_int, std::ptr::null_mut());
            anyhow::ensure!(!mat.is_null(), "failed to allocate input");
            for ch in 0..c {
                let dst = ncnn_mat_get_channel_data(mat, ch as c_int) as *mut f32;
                std::ptr::copy_nonoverlapping(input[ch * h * w..].as_ptr(), dst, h * w);
            }
            drop(input);
            let ex = ncnn_extractor_create(self.net);
            let result = extract(ex, &in0, mat, self.outputs);
            ncnn_extractor_destroy(ex);
            ncnn_mat_destroy(mat);
            result
        }
    }
}

unsafe fn extract(ex: Ptr, in0: &CString, mat: Ptr, count: usize) -> anyhow::Result<Vec<Output>> {
    anyhow::ensure!(
        ncnn_extractor_input(ex, in0.as_ptr(), mat) == 0,
        "failed to set input"
    );
    let mut outputs = Vec::with_capacity(count);
    for k in 0..count {
        let name = CString::new(format!("out{k}"))?;
        let mut out: Ptr = std::ptr::null_mut();
        anyhow::ensure!(
            ncnn_extractor_extract(ex, name.as_ptr(), &mut out) == 0 && !out.is_null(),
            "failed to extract out{k}"
        );
        let output = read_output(out);
        ncnn_mat_destroy(out);
        outputs.push(output?);
    }
    Ok(outputs)
}

unsafe fn read_output(mat: Ptr) -> anyhow::Result<Output> {
    anyhow::ensure!(
        ncnn_mat_get_elempack(mat) == 1,
        "packed output is not supported"
    );
    let (w, h, c) = (
        ncnn_mat_get_w(mat) as usize,
        ncnn_mat_get_h(mat) as usize,
        ncnn_mat_get_c(mat) as usize,
    );
    let shape = match ncnn_mat_get_dims(mat) {
        1 => vec![w],
        2 => vec![h, w],
        3 => vec![c, h, w],
        d => anyhow::bail!("unsupported output dims {d}"),
    };
    let mut data = Vec::with_capacity(w * h * c);
    for ch in 0..c {
        let src = ncnn_mat_get_channel_data(mat, ch as c_int) as *const f32;
        data.extend_from_slice(std::slice::from_raw_parts(src, w * h));
    }
    Ok(Output { shape, data })
}

fn output_count(param: &str) -> usize {
    (0..)
        .take_while(|k| {
            let name = format!("out{k}");
            param.split_whitespace().any(|t| t == name)
        })
        .count()
}

impl Drop for NcnnModel {
    fn drop(&mut self) {
        unsafe {
            ncnn_net_destroy(self.net);
            ncnn_option_destroy(self.opt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vision-ncnn-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 出力の数は、out0 から順に名前が続く数として数えること
    #[test]
    fn test_output_count_follows_consecutive_names() {
        assert_eq!(
            output_count("Input in0 0 1 in0\nSigmoid s 1 1 in0 out0\n"),
            1
        );
        assert_eq!(
            output_count("A a 1 1 in0 out0\nB b 1 1 in0 out1\nC c 1 1 in0 out3\n"),
            2
        );
        assert_eq!(output_count("Input in0 0 1 in0\n"), 0);
    }

    /// 足し算も掛け算もしない最小のモデルに通すと、入力がそのまま形も含めて出てくること（データの詰め方と取り出し方の確認）
    #[test]
    fn test_identity_model_returns_input() {
        let dir = temp_dir("identity");
        let param = write(
            &dir,
            "id.param",
            "7767517\n2 2\nInput in0 0 1 in0\nNoop noop 1 1 in0 out0\n",
        );
        let bin = write(&dir, "id.bin", "");
        let model = NcnnModel::load(&param, &bin).unwrap();
        let input: Vec<f32> = (0..24).map(|v| v as f32).collect();

        let out = model.run(input.clone(), [2, 3, 4]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].shape, vec![2, 3, 4]);
        assert_eq!(out[0].data, input);
    }

    /// 入力の数が形と合わないときは、計算せずに失敗にすること
    #[test]
    fn test_input_size_must_match_shape() {
        let dir = temp_dir("mismatch");
        let param = write(
            &dir,
            "id.param",
            "7767517\n2 2\nInput in0 0 1 in0\nNoop noop 1 1 in0 out0\n",
        );
        let bin = write(&dir, "id.bin", "");
        let model = NcnnModel::load(&param, &bin).unwrap();

        let result = model.run(vec![0.0; 5], [1, 2, 3]);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(result.is_err());
    }

    /// 存在しないモデルのファイルは、読み込みを失敗にすること
    #[test]
    fn test_missing_file_fails_to_load() {
        assert!(NcnnModel::load(
            Path::new("/nonexistent.param"),
            Path::new("/nonexistent.bin")
        )
        .is_err());
    }
}
