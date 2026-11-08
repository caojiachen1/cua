// Generated from the pinned OmniParser YOLOv9-E ONNX graph by burn-onnx.
extern crate alloc;
use burn::prelude::*;
use burn::nn::PaddingConfig2d;
use burn::nn::PaddingConfig3d;
use burn::nn::conv::Conv2d;
use burn::nn::conv::Conv2dConfig;
use burn::nn::conv::Conv3d;
use burn::nn::conv::Conv3dConfig;
use burn::nn::pool::AvgPool2d;
use burn::nn::pool::AvgPool2dConfig;
use burn::nn::pool::MaxPool2d;
use burn::nn::pool::MaxPool2dConfig;
use burn::tensor::Bytes;
use burn_store::BurnpackStore;
use burn_store::ModuleSnapshot;


#[derive(Module, Debug)]
pub struct Submodule1 {
    conv2d1: Conv2d,
    conv2d2: Conv2d,
    conv2d3: Conv2d,
    conv2d4: Conv2d,
    conv2d5: Conv2d,
    conv2d6: Conv2d,
    conv2d7: Conv2d,
    conv2d8: Conv2d,
    conv2d9: Conv2d,
    conv2d10: Conv2d,
    conv2d11: Conv2d,
    conv2d12: Conv2d,
    conv2d13: Conv2d,
    conv2d14: Conv2d,
    conv2d15: Conv2d,
    conv2d16: Conv2d,
    conv2d17: Conv2d,
    conv2d18: Conv2d,
    conv2d19: Conv2d,
    conv2d20: Conv2d,
    conv2d21: Conv2d,
    conv2d22: Conv2d,
    conv2d23: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule1 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d1 = Conv2dConfig::new([3, 64], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d2 = Conv2dConfig::new([64, 128], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d3 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d4 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d5 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d6 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d7 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d8 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d9 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d10 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d11 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d12 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d13 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d14 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d15 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d16 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d17 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d18 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d19 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d20 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d21 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d22 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d23 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d1,
            conv2d2,
            conv2d3,
            conv2d4,
            conv2d5,
            conv2d6,
            conv2d7,
            conv2d8,
            conv2d9,
            conv2d10,
            conv2d11,
            conv2d12,
            conv2d13,
            conv2d14,
            conv2d15,
            conv2d16,
            conv2d17,
            conv2d18,
            conv2d19,
            conv2d20,
            conv2d21,
            conv2d22,
            conv2d23,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, images: Tensor<4>) -> (Tensor<4>, Tensor<4>) {
        let conv2d1_out1 = self.conv2d1.forward(images);
        let sigmoid1_out1 = burn::tensor::activation::sigmoid(conv2d1_out1.clone());
        let mul1_out1 = conv2d1_out1.mul(sigmoid1_out1);
        let conv2d2_out1 = self.conv2d2.forward(mul1_out1.clone());
        let sigmoid2_out1 = burn::tensor::activation::sigmoid(conv2d2_out1.clone());
        let mul2_out1 = conv2d2_out1.mul(sigmoid2_out1);
        let conv2d3_out1 = self.conv2d3.forward(mul2_out1);
        let sigmoid3_out1 = burn::tensor::activation::sigmoid(conv2d3_out1.clone());
        let mul3_out1 = conv2d3_out1.mul(sigmoid3_out1);
        let slice1_out1 = mul3_out1.clone().slice(s![.., 0..64, .., ..]);
        let slice2_out1 = mul3_out1.slice(s![.., 64..128, .., ..]);
        let conv2d4_out1 = self.conv2d4.forward(slice2_out1.clone());
        let sigmoid4_out1 = burn::tensor::activation::sigmoid(conv2d4_out1.clone());
        let mul6_out1 = conv2d4_out1.mul(sigmoid4_out1);
        let conv2d5_out1 = self.conv2d5.forward(mul6_out1.clone());
        let conv2d6_out1 = self.conv2d6.forward(mul6_out1.clone());
        let add2_out1 = conv2d5_out1.add(conv2d6_out1);
        let sigmoid5_out1 = burn::tensor::activation::sigmoid(add2_out1.clone());
        let mul7_out1 = add2_out1.mul(sigmoid5_out1);
        let conv2d7_out1 = self.conv2d7.forward(mul7_out1);
        let sigmoid6_out1 = burn::tensor::activation::sigmoid(conv2d7_out1.clone());
        let mul8_out1 = conv2d7_out1.mul(sigmoid6_out1);
        let add3_out1 = mul6_out1.add(mul8_out1);
        let conv2d8_out1 = self.conv2d8.forward(add3_out1.clone());
        let conv2d9_out1 = self.conv2d9.forward(add3_out1.clone());
        let add4_out1 = conv2d8_out1.add(conv2d9_out1);
        let sigmoid7_out1 = burn::tensor::activation::sigmoid(add4_out1.clone());
        let mul9_out1 = add4_out1.mul(sigmoid7_out1);
        let conv2d10_out1 = self.conv2d10.forward(mul9_out1);
        let sigmoid8_out1 = burn::tensor::activation::sigmoid(conv2d10_out1.clone());
        let mul10_out1 = conv2d10_out1.mul(sigmoid8_out1);
        let add5_out1 = add3_out1.add(mul10_out1);
        let conv2d11_out1 = self.conv2d11.forward(slice2_out1.clone());
        let sigmoid9_out1 = burn::tensor::activation::sigmoid(conv2d11_out1.clone());
        let mul11_out1 = conv2d11_out1.mul(sigmoid9_out1);
        let concat1_out1 = burn::tensor::Tensor::cat([add5_out1, mul11_out1].into(), 1);
        let conv2d12_out1 = self.conv2d12.forward(concat1_out1);
        let sigmoid10_out1 = burn::tensor::activation::sigmoid(conv2d12_out1.clone());
        let mul12_out1 = conv2d12_out1.mul(sigmoid10_out1);
        let conv2d13_out1 = self.conv2d13.forward(mul12_out1);
        let sigmoid11_out1 = burn::tensor::activation::sigmoid(conv2d13_out1.clone());
        let mul13_out1 = conv2d13_out1.mul(sigmoid11_out1);
        let conv2d14_out1 = self.conv2d14.forward(mul13_out1.clone());
        let sigmoid12_out1 = burn::tensor::activation::sigmoid(conv2d14_out1.clone());
        let mul14_out1 = conv2d14_out1.mul(sigmoid12_out1);
        let conv2d15_out1 = self.conv2d15.forward(mul14_out1.clone());
        let conv2d16_out1 = self.conv2d16.forward(mul14_out1.clone());
        let add6_out1 = conv2d15_out1.add(conv2d16_out1);
        let sigmoid13_out1 = burn::tensor::activation::sigmoid(add6_out1.clone());
        let mul15_out1 = add6_out1.mul(sigmoid13_out1);
        let conv2d17_out1 = self.conv2d17.forward(mul15_out1);
        let sigmoid14_out1 = burn::tensor::activation::sigmoid(conv2d17_out1.clone());
        let mul16_out1 = conv2d17_out1.mul(sigmoid14_out1);
        let add7_out1 = mul14_out1.add(mul16_out1);
        let conv2d18_out1 = self.conv2d18.forward(add7_out1.clone());
        let conv2d19_out1 = self.conv2d19.forward(add7_out1.clone());
        let add8_out1 = conv2d18_out1.add(conv2d19_out1);
        let sigmoid15_out1 = burn::tensor::activation::sigmoid(add8_out1.clone());
        let mul17_out1 = add8_out1.mul(sigmoid15_out1);
        let conv2d20_out1 = self.conv2d20.forward(mul17_out1);
        let sigmoid16_out1 = burn::tensor::activation::sigmoid(conv2d20_out1.clone());
        let mul18_out1 = conv2d20_out1.mul(sigmoid16_out1);
        let add9_out1 = add7_out1.add(mul18_out1);
        let conv2d21_out1 = self.conv2d21.forward(mul13_out1.clone());
        let sigmoid17_out1 = burn::tensor::activation::sigmoid(conv2d21_out1.clone());
        let mul19_out1 = conv2d21_out1.mul(sigmoid17_out1);
        let concat2_out1 = burn::tensor::Tensor::cat([add9_out1, mul19_out1].into(), 1);
        let conv2d22_out1 = self.conv2d22.forward(concat2_out1);
        let sigmoid18_out1 = burn::tensor::activation::sigmoid(conv2d22_out1.clone());
        let mul20_out1 = conv2d22_out1.mul(sigmoid18_out1);
        let conv2d23_out1 = self.conv2d23.forward(mul20_out1);
        let sigmoid19_out1 = burn::tensor::activation::sigmoid(conv2d23_out1.clone());
        let mul21_out1 = conv2d23_out1.mul(sigmoid19_out1);
        let concat3_out1 = burn::tensor::Tensor::cat(
            [slice1_out1, slice2_out1, mul13_out1, mul21_out1].into(),
            1,
        );
        (concat3_out1, mul1_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule2 {
    conv2d24: Conv2d,
    averagepool2d1: AvgPool2d,
    conv2d25: Conv2d,
    maxpool2d1: MaxPool2d,
    conv2d26: Conv2d,
    conv2d27: Conv2d,
    conv2d28: Conv2d,
    conv2d29: Conv2d,
    conv2d30: Conv2d,
    conv2d31: Conv2d,
    conv2d32: Conv2d,
    conv2d33: Conv2d,
    conv2d34: Conv2d,
    conv2d35: Conv2d,
    conv2d36: Conv2d,
    conv2d37: Conv2d,
    conv2d38: Conv2d,
    conv2d39: Conv2d,
    conv2d40: Conv2d,
    conv2d41: Conv2d,
    conv2d42: Conv2d,
    conv2d43: Conv2d,
    conv2d44: Conv2d,
    conv2d45: Conv2d,
    conv2d46: Conv2d,
    conv2d47: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule2 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d24 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d1 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d25 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d1 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d26 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d27 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d28 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d29 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d30 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d31 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d32 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d33 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d34 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d35 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d36 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d37 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d38 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d39 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d40 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d41 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d42 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d43 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d44 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d45 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d46 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d47 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d24,
            averagepool2d1,
            conv2d25,
            maxpool2d1,
            conv2d26,
            conv2d27,
            conv2d28,
            conv2d29,
            conv2d30,
            conv2d31,
            conv2d32,
            conv2d33,
            conv2d34,
            conv2d35,
            conv2d36,
            conv2d37,
            conv2d38,
            conv2d39,
            conv2d40,
            conv2d41,
            conv2d42,
            conv2d43,
            conv2d44,
            conv2d45,
            conv2d46,
            conv2d47,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, concat3_out1: Tensor<4>) -> (Tensor<4>, Tensor<4>) {
        let conv2d24_out1 = self.conv2d24.forward(concat3_out1);
        let sigmoid20_out1 = burn::tensor::activation::sigmoid(conv2d24_out1.clone());
        let mul22_out1 = conv2d24_out1.mul(sigmoid20_out1);
        let averagepool2d1_out1 = self.averagepool2d1.forward(mul22_out1.clone());
        let shape2_out1: [i64; 4] = {
            let axes = &averagepool2d1_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant633_out1: [i64; 1] = [1i64];
        let gather2_out1: [i64; 1usize] = constant633_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape2_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape2_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant635_out1: [i64; 1] = [1i64];
        let add10_out1 = {
            let __lhs = gather2_out1;
            let __rhs = constant635_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant636_out1: [i64; 1] = [2i64];
        let div2_out1 = {
            let __lhs = add10_out1;
            let __rhs = constant636_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice3_out1 = averagepool2d1_out1
            .clone()
            .slice(s![.., 0..div2_out1[0], .., ..]);
        let constant638_out1: [i64; 1] = [2i64];
        let mul24_out1 = {
            let __lhs = div2_out1;
            let __rhs = constant638_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice4_out1 = averagepool2d1_out1
            .slice(s![.., div2_out1[0]..mul24_out1[0], .., ..]);
        let conv2d25_out1 = self.conv2d25.forward(slice3_out1);
        let sigmoid21_out1 = burn::tensor::activation::sigmoid(conv2d25_out1.clone());
        let mul25_out1 = conv2d25_out1.mul(sigmoid21_out1);
        let maxpool2d1_out1 = self.maxpool2d1.forward(slice4_out1);
        let conv2d26_out1 = self.conv2d26.forward(maxpool2d1_out1);
        let sigmoid22_out1 = burn::tensor::activation::sigmoid(conv2d26_out1.clone());
        let mul26_out1 = conv2d26_out1.mul(sigmoid22_out1);
        let concat4_out1 = burn::tensor::Tensor::cat([mul25_out1, mul26_out1].into(), 1);
        let conv2d27_out1 = self.conv2d27.forward(concat4_out1);
        let sigmoid23_out1 = burn::tensor::activation::sigmoid(conv2d27_out1.clone());
        let mul27_out1 = conv2d27_out1.mul(sigmoid23_out1);
        let shape3_out1: [i64; 4] = {
            let axes = &mul27_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant639_out1: [i64; 1] = [1i64];
        let gather3_out1: [i64; 1usize] = constant639_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape3_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape3_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant641_out1: [i64; 1] = [1i64];
        let add11_out1 = {
            let __lhs = gather3_out1;
            let __rhs = constant641_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant642_out1: [i64; 1] = [2i64];
        let div3_out1 = {
            let __lhs = add11_out1;
            let __rhs = constant642_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice5_out1 = mul27_out1.clone().slice(s![.., 0..div3_out1[0], .., ..]);
        let constant644_out1: [i64; 1] = [2i64];
        let mul29_out1 = {
            let __lhs = div3_out1;
            let __rhs = constant644_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice6_out1 = mul27_out1.slice(s![.., div3_out1[0]..mul29_out1[0], .., ..]);
        let conv2d28_out1 = self.conv2d28.forward(slice6_out1.clone());
        let sigmoid24_out1 = burn::tensor::activation::sigmoid(conv2d28_out1.clone());
        let mul30_out1 = conv2d28_out1.mul(sigmoid24_out1);
        let conv2d29_out1 = self.conv2d29.forward(mul30_out1.clone());
        let conv2d30_out1 = self.conv2d30.forward(mul30_out1.clone());
        let add12_out1 = conv2d29_out1.add(conv2d30_out1);
        let sigmoid25_out1 = burn::tensor::activation::sigmoid(add12_out1.clone());
        let mul31_out1 = add12_out1.mul(sigmoid25_out1);
        let conv2d31_out1 = self.conv2d31.forward(mul31_out1);
        let sigmoid26_out1 = burn::tensor::activation::sigmoid(conv2d31_out1.clone());
        let mul32_out1 = conv2d31_out1.mul(sigmoid26_out1);
        let add13_out1 = mul30_out1.add(mul32_out1);
        let conv2d32_out1 = self.conv2d32.forward(add13_out1.clone());
        let conv2d33_out1 = self.conv2d33.forward(add13_out1.clone());
        let add14_out1 = conv2d32_out1.add(conv2d33_out1);
        let sigmoid27_out1 = burn::tensor::activation::sigmoid(add14_out1.clone());
        let mul33_out1 = add14_out1.mul(sigmoid27_out1);
        let conv2d34_out1 = self.conv2d34.forward(mul33_out1);
        let sigmoid28_out1 = burn::tensor::activation::sigmoid(conv2d34_out1.clone());
        let mul34_out1 = conv2d34_out1.mul(sigmoid28_out1);
        let add15_out1 = add13_out1.add(mul34_out1);
        let conv2d35_out1 = self.conv2d35.forward(slice6_out1.clone());
        let sigmoid29_out1 = burn::tensor::activation::sigmoid(conv2d35_out1.clone());
        let mul35_out1 = conv2d35_out1.mul(sigmoid29_out1);
        let concat5_out1 = burn::tensor::Tensor::cat([add15_out1, mul35_out1].into(), 1);
        let conv2d36_out1 = self.conv2d36.forward(concat5_out1);
        let sigmoid30_out1 = burn::tensor::activation::sigmoid(conv2d36_out1.clone());
        let mul36_out1 = conv2d36_out1.mul(sigmoid30_out1);
        let conv2d37_out1 = self.conv2d37.forward(mul36_out1);
        let sigmoid31_out1 = burn::tensor::activation::sigmoid(conv2d37_out1.clone());
        let mul37_out1 = conv2d37_out1.mul(sigmoid31_out1);
        let conv2d38_out1 = self.conv2d38.forward(mul37_out1.clone());
        let sigmoid32_out1 = burn::tensor::activation::sigmoid(conv2d38_out1.clone());
        let mul38_out1 = conv2d38_out1.mul(sigmoid32_out1);
        let conv2d39_out1 = self.conv2d39.forward(mul38_out1.clone());
        let conv2d40_out1 = self.conv2d40.forward(mul38_out1.clone());
        let add16_out1 = conv2d39_out1.add(conv2d40_out1);
        let sigmoid33_out1 = burn::tensor::activation::sigmoid(add16_out1.clone());
        let mul39_out1 = add16_out1.mul(sigmoid33_out1);
        let conv2d41_out1 = self.conv2d41.forward(mul39_out1);
        let sigmoid34_out1 = burn::tensor::activation::sigmoid(conv2d41_out1.clone());
        let mul40_out1 = conv2d41_out1.mul(sigmoid34_out1);
        let add17_out1 = mul38_out1.add(mul40_out1);
        let conv2d42_out1 = self.conv2d42.forward(add17_out1.clone());
        let conv2d43_out1 = self.conv2d43.forward(add17_out1.clone());
        let add18_out1 = conv2d42_out1.add(conv2d43_out1);
        let sigmoid35_out1 = burn::tensor::activation::sigmoid(add18_out1.clone());
        let mul41_out1 = add18_out1.mul(sigmoid35_out1);
        let conv2d44_out1 = self.conv2d44.forward(mul41_out1);
        let sigmoid36_out1 = burn::tensor::activation::sigmoid(conv2d44_out1.clone());
        let mul42_out1 = conv2d44_out1.mul(sigmoid36_out1);
        let add19_out1 = add17_out1.add(mul42_out1);
        let conv2d45_out1 = self.conv2d45.forward(mul37_out1.clone());
        let sigmoid37_out1 = burn::tensor::activation::sigmoid(conv2d45_out1.clone());
        let mul43_out1 = conv2d45_out1.mul(sigmoid37_out1);
        let concat6_out1 = burn::tensor::Tensor::cat([add19_out1, mul43_out1].into(), 1);
        let conv2d46_out1 = self.conv2d46.forward(concat6_out1);
        let sigmoid38_out1 = burn::tensor::activation::sigmoid(conv2d46_out1.clone());
        let mul44_out1 = conv2d46_out1.mul(sigmoid38_out1);
        let conv2d47_out1 = self.conv2d47.forward(mul44_out1);
        let sigmoid39_out1 = burn::tensor::activation::sigmoid(conv2d47_out1.clone());
        let mul45_out1 = conv2d47_out1.mul(sigmoid39_out1);
        let concat7_out1 = burn::tensor::Tensor::cat(
            [slice5_out1, slice6_out1, mul37_out1, mul45_out1].into(),
            1,
        );
        (concat7_out1, mul22_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule3 {
    conv2d48: Conv2d,
    averagepool2d2: AvgPool2d,
    conv2d49: Conv2d,
    maxpool2d2: MaxPool2d,
    conv2d50: Conv2d,
    conv2d51: Conv2d,
    conv2d52: Conv2d,
    conv2d53: Conv2d,
    conv2d54: Conv2d,
    conv2d55: Conv2d,
    conv2d56: Conv2d,
    conv2d57: Conv2d,
    conv2d58: Conv2d,
    conv2d59: Conv2d,
    conv2d60: Conv2d,
    conv2d61: Conv2d,
    conv2d62: Conv2d,
    conv2d63: Conv2d,
    conv2d64: Conv2d,
    conv2d65: Conv2d,
    conv2d66: Conv2d,
    conv2d67: Conv2d,
    conv2d68: Conv2d,
    conv2d69: Conv2d,
    conv2d70: Conv2d,
    conv2d71: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule3 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d48 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d2 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d49 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d2 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d50 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d51 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d52 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d53 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d54 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d55 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d56 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d57 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d58 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d59 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d60 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d61 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d62 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d63 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d64 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d65 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d66 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d67 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d68 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d69 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d70 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d71 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d48,
            averagepool2d2,
            conv2d49,
            maxpool2d2,
            conv2d50,
            conv2d51,
            conv2d52,
            conv2d53,
            conv2d54,
            conv2d55,
            conv2d56,
            conv2d57,
            conv2d58,
            conv2d59,
            conv2d60,
            conv2d61,
            conv2d62,
            conv2d63,
            conv2d64,
            conv2d65,
            conv2d66,
            conv2d67,
            conv2d68,
            conv2d69,
            conv2d70,
            conv2d71,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, concat7_out1: Tensor<4>) -> (Tensor<4>, Tensor<4>) {
        let conv2d48_out1 = self.conv2d48.forward(concat7_out1);
        let sigmoid40_out1 = burn::tensor::activation::sigmoid(conv2d48_out1.clone());
        let mul46_out1 = conv2d48_out1.mul(sigmoid40_out1);
        let averagepool2d2_out1 = self.averagepool2d2.forward(mul46_out1.clone());
        let shape4_out1: [i64; 4] = {
            let axes = &averagepool2d2_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant645_out1: [i64; 1] = [1i64];
        let gather4_out1: [i64; 1usize] = constant645_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape4_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape4_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant647_out1: [i64; 1] = [1i64];
        let add20_out1 = {
            let __lhs = gather4_out1;
            let __rhs = constant647_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant648_out1: [i64; 1] = [2i64];
        let div4_out1 = {
            let __lhs = add20_out1;
            let __rhs = constant648_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice7_out1 = averagepool2d2_out1
            .clone()
            .slice(s![.., 0..div4_out1[0], .., ..]);
        let constant650_out1: [i64; 1] = [2i64];
        let mul48_out1 = {
            let __lhs = div4_out1;
            let __rhs = constant650_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice8_out1 = averagepool2d2_out1
            .slice(s![.., div4_out1[0]..mul48_out1[0], .., ..]);
        let conv2d49_out1 = self.conv2d49.forward(slice7_out1);
        let sigmoid41_out1 = burn::tensor::activation::sigmoid(conv2d49_out1.clone());
        let mul49_out1 = conv2d49_out1.mul(sigmoid41_out1);
        let maxpool2d2_out1 = self.maxpool2d2.forward(slice8_out1);
        let conv2d50_out1 = self.conv2d50.forward(maxpool2d2_out1);
        let sigmoid42_out1 = burn::tensor::activation::sigmoid(conv2d50_out1.clone());
        let mul50_out1 = conv2d50_out1.mul(sigmoid42_out1);
        let concat8_out1 = burn::tensor::Tensor::cat([mul49_out1, mul50_out1].into(), 1);
        let conv2d51_out1 = self.conv2d51.forward(concat8_out1);
        let sigmoid43_out1 = burn::tensor::activation::sigmoid(conv2d51_out1.clone());
        let mul51_out1 = conv2d51_out1.mul(sigmoid43_out1);
        let shape5_out1: [i64; 4] = {
            let axes = &mul51_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant651_out1: [i64; 1] = [1i64];
        let gather5_out1: [i64; 1usize] = constant651_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape5_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape5_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant653_out1: [i64; 1] = [1i64];
        let add21_out1 = {
            let __lhs = gather5_out1;
            let __rhs = constant653_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant654_out1: [i64; 1] = [2i64];
        let div5_out1 = {
            let __lhs = add21_out1;
            let __rhs = constant654_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice9_out1 = mul51_out1.clone().slice(s![.., 0..div5_out1[0], .., ..]);
        let constant656_out1: [i64; 1] = [2i64];
        let mul53_out1 = {
            let __lhs = div5_out1;
            let __rhs = constant656_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice10_out1 = mul51_out1.slice(s![.., div5_out1[0]..mul53_out1[0], .., ..]);
        let conv2d52_out1 = self.conv2d52.forward(slice10_out1.clone());
        let sigmoid44_out1 = burn::tensor::activation::sigmoid(conv2d52_out1.clone());
        let mul54_out1 = conv2d52_out1.mul(sigmoid44_out1);
        let conv2d53_out1 = self.conv2d53.forward(mul54_out1.clone());
        let conv2d54_out1 = self.conv2d54.forward(mul54_out1.clone());
        let add22_out1 = conv2d53_out1.add(conv2d54_out1);
        let sigmoid45_out1 = burn::tensor::activation::sigmoid(add22_out1.clone());
        let mul55_out1 = add22_out1.mul(sigmoid45_out1);
        let conv2d55_out1 = self.conv2d55.forward(mul55_out1);
        let sigmoid46_out1 = burn::tensor::activation::sigmoid(conv2d55_out1.clone());
        let mul56_out1 = conv2d55_out1.mul(sigmoid46_out1);
        let add23_out1 = mul54_out1.add(mul56_out1);
        let conv2d56_out1 = self.conv2d56.forward(add23_out1.clone());
        let conv2d57_out1 = self.conv2d57.forward(add23_out1.clone());
        let add24_out1 = conv2d56_out1.add(conv2d57_out1);
        let sigmoid47_out1 = burn::tensor::activation::sigmoid(add24_out1.clone());
        let mul57_out1 = add24_out1.mul(sigmoid47_out1);
        let conv2d58_out1 = self.conv2d58.forward(mul57_out1);
        let sigmoid48_out1 = burn::tensor::activation::sigmoid(conv2d58_out1.clone());
        let mul58_out1 = conv2d58_out1.mul(sigmoid48_out1);
        let add25_out1 = add23_out1.add(mul58_out1);
        let conv2d59_out1 = self.conv2d59.forward(slice10_out1.clone());
        let sigmoid49_out1 = burn::tensor::activation::sigmoid(conv2d59_out1.clone());
        let mul59_out1 = conv2d59_out1.mul(sigmoid49_out1);
        let concat9_out1 = burn::tensor::Tensor::cat([add25_out1, mul59_out1].into(), 1);
        let conv2d60_out1 = self.conv2d60.forward(concat9_out1);
        let sigmoid50_out1 = burn::tensor::activation::sigmoid(conv2d60_out1.clone());
        let mul60_out1 = conv2d60_out1.mul(sigmoid50_out1);
        let conv2d61_out1 = self.conv2d61.forward(mul60_out1);
        let sigmoid51_out1 = burn::tensor::activation::sigmoid(conv2d61_out1.clone());
        let mul61_out1 = conv2d61_out1.mul(sigmoid51_out1);
        let conv2d62_out1 = self.conv2d62.forward(mul61_out1.clone());
        let sigmoid52_out1 = burn::tensor::activation::sigmoid(conv2d62_out1.clone());
        let mul62_out1 = conv2d62_out1.mul(sigmoid52_out1);
        let conv2d63_out1 = self.conv2d63.forward(mul62_out1.clone());
        let conv2d64_out1 = self.conv2d64.forward(mul62_out1.clone());
        let add26_out1 = conv2d63_out1.add(conv2d64_out1);
        let sigmoid53_out1 = burn::tensor::activation::sigmoid(add26_out1.clone());
        let mul63_out1 = add26_out1.mul(sigmoid53_out1);
        let conv2d65_out1 = self.conv2d65.forward(mul63_out1);
        let sigmoid54_out1 = burn::tensor::activation::sigmoid(conv2d65_out1.clone());
        let mul64_out1 = conv2d65_out1.mul(sigmoid54_out1);
        let add27_out1 = mul62_out1.add(mul64_out1);
        let conv2d66_out1 = self.conv2d66.forward(add27_out1.clone());
        let conv2d67_out1 = self.conv2d67.forward(add27_out1.clone());
        let add28_out1 = conv2d66_out1.add(conv2d67_out1);
        let sigmoid55_out1 = burn::tensor::activation::sigmoid(add28_out1.clone());
        let mul65_out1 = add28_out1.mul(sigmoid55_out1);
        let conv2d68_out1 = self.conv2d68.forward(mul65_out1);
        let sigmoid56_out1 = burn::tensor::activation::sigmoid(conv2d68_out1.clone());
        let mul66_out1 = conv2d68_out1.mul(sigmoid56_out1);
        let add29_out1 = add27_out1.add(mul66_out1);
        let conv2d69_out1 = self.conv2d69.forward(mul61_out1.clone());
        let sigmoid57_out1 = burn::tensor::activation::sigmoid(conv2d69_out1.clone());
        let mul67_out1 = conv2d69_out1.mul(sigmoid57_out1);
        let concat10_out1 = burn::tensor::Tensor::cat(
            [add29_out1, mul67_out1].into(),
            1,
        );
        let conv2d70_out1 = self.conv2d70.forward(concat10_out1);
        let sigmoid58_out1 = burn::tensor::activation::sigmoid(conv2d70_out1.clone());
        let mul68_out1 = conv2d70_out1.mul(sigmoid58_out1);
        let conv2d71_out1 = self.conv2d71.forward(mul68_out1);
        let sigmoid59_out1 = burn::tensor::activation::sigmoid(conv2d71_out1.clone());
        let mul69_out1 = conv2d71_out1.mul(sigmoid59_out1);
        let concat11_out1 = burn::tensor::Tensor::cat(
            [slice9_out1, slice10_out1, mul61_out1, mul69_out1].into(),
            1,
        );
        (concat11_out1, mul46_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule4 {
    conv2d72: Conv2d,
    averagepool2d3: AvgPool2d,
    conv2d73: Conv2d,
    maxpool2d3: MaxPool2d,
    conv2d74: Conv2d,
    conv2d75: Conv2d,
    conv2d76: Conv2d,
    conv2d77: Conv2d,
    conv2d78: Conv2d,
    conv2d79: Conv2d,
    conv2d80: Conv2d,
    conv2d81: Conv2d,
    conv2d82: Conv2d,
    conv2d83: Conv2d,
    conv2d84: Conv2d,
    conv2d85: Conv2d,
    conv2d86: Conv2d,
    conv2d87: Conv2d,
    conv2d88: Conv2d,
    conv2d89: Conv2d,
    conv2d90: Conv2d,
    conv2d91: Conv2d,
    conv2d92: Conv2d,
    conv2d93: Conv2d,
    conv2d94: Conv2d,
    conv2d95: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule4 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d72 = Conv2dConfig::new([1024, 1024], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d3 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d73 = Conv2dConfig::new([512, 512], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d3 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d74 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d75 = Conv2dConfig::new([1024, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d76 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d77 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d78 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d79 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d80 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d81 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d82 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d83 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d84 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d85 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d86 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d87 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d88 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d89 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d90 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d91 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d92 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d93 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d94 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d95 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d72,
            averagepool2d3,
            conv2d73,
            maxpool2d3,
            conv2d74,
            conv2d75,
            conv2d76,
            conv2d77,
            conv2d78,
            conv2d79,
            conv2d80,
            conv2d81,
            conv2d82,
            conv2d83,
            conv2d84,
            conv2d85,
            conv2d86,
            conv2d87,
            conv2d88,
            conv2d89,
            conv2d90,
            conv2d91,
            conv2d92,
            conv2d93,
            conv2d94,
            conv2d95,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, concat11_out1: Tensor<4>) -> (Tensor<4>, Tensor<4>) {
        let conv2d72_out1 = self.conv2d72.forward(concat11_out1);
        let sigmoid60_out1 = burn::tensor::activation::sigmoid(conv2d72_out1.clone());
        let mul70_out1 = conv2d72_out1.mul(sigmoid60_out1);
        let averagepool2d3_out1 = self.averagepool2d3.forward(mul70_out1.clone());
        let shape6_out1: [i64; 4] = {
            let axes = &averagepool2d3_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant657_out1: [i64; 1] = [1i64];
        let gather6_out1: [i64; 1usize] = constant657_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape6_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape6_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant659_out1: [i64; 1] = [1i64];
        let add30_out1 = {
            let __lhs = gather6_out1;
            let __rhs = constant659_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant660_out1: [i64; 1] = [2i64];
        let div6_out1 = {
            let __lhs = add30_out1;
            let __rhs = constant660_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice11_out1 = averagepool2d3_out1
            .clone()
            .slice(s![.., 0..div6_out1[0], .., ..]);
        let constant662_out1: [i64; 1] = [2i64];
        let mul72_out1 = {
            let __lhs = div6_out1;
            let __rhs = constant662_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice12_out1 = averagepool2d3_out1
            .slice(s![.., div6_out1[0]..mul72_out1[0], .., ..]);
        let conv2d73_out1 = self.conv2d73.forward(slice11_out1);
        let sigmoid61_out1 = burn::tensor::activation::sigmoid(conv2d73_out1.clone());
        let mul73_out1 = conv2d73_out1.mul(sigmoid61_out1);
        let maxpool2d3_out1 = self.maxpool2d3.forward(slice12_out1);
        let conv2d74_out1 = self.conv2d74.forward(maxpool2d3_out1);
        let sigmoid62_out1 = burn::tensor::activation::sigmoid(conv2d74_out1.clone());
        let mul74_out1 = conv2d74_out1.mul(sigmoid62_out1);
        let concat12_out1 = burn::tensor::Tensor::cat(
            [mul73_out1, mul74_out1].into(),
            1,
        );
        let conv2d75_out1 = self.conv2d75.forward(concat12_out1);
        let sigmoid63_out1 = burn::tensor::activation::sigmoid(conv2d75_out1.clone());
        let mul75_out1 = conv2d75_out1.mul(sigmoid63_out1);
        let shape7_out1: [i64; 4] = {
            let axes = &mul75_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant663_out1: [i64; 1] = [1i64];
        let gather7_out1: [i64; 1usize] = constant663_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape7_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape7_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant665_out1: [i64; 1] = [1i64];
        let add31_out1 = {
            let __lhs = gather7_out1;
            let __rhs = constant665_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant666_out1: [i64; 1] = [2i64];
        let div7_out1 = {
            let __lhs = add31_out1;
            let __rhs = constant666_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice13_out1 = mul75_out1.clone().slice(s![.., 0..div7_out1[0], .., ..]);
        let constant668_out1: [i64; 1] = [2i64];
        let mul77_out1 = {
            let __lhs = div7_out1;
            let __rhs = constant668_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice14_out1 = mul75_out1.slice(s![.., div7_out1[0]..mul77_out1[0], .., ..]);
        let conv2d76_out1 = self.conv2d76.forward(slice14_out1.clone());
        let sigmoid64_out1 = burn::tensor::activation::sigmoid(conv2d76_out1.clone());
        let mul78_out1 = conv2d76_out1.mul(sigmoid64_out1);
        let conv2d77_out1 = self.conv2d77.forward(mul78_out1.clone());
        let conv2d78_out1 = self.conv2d78.forward(mul78_out1.clone());
        let add32_out1 = conv2d77_out1.add(conv2d78_out1);
        let sigmoid65_out1 = burn::tensor::activation::sigmoid(add32_out1.clone());
        let mul79_out1 = add32_out1.mul(sigmoid65_out1);
        let conv2d79_out1 = self.conv2d79.forward(mul79_out1);
        let sigmoid66_out1 = burn::tensor::activation::sigmoid(conv2d79_out1.clone());
        let mul80_out1 = conv2d79_out1.mul(sigmoid66_out1);
        let add33_out1 = mul78_out1.add(mul80_out1);
        let conv2d80_out1 = self.conv2d80.forward(add33_out1.clone());
        let conv2d81_out1 = self.conv2d81.forward(add33_out1.clone());
        let add34_out1 = conv2d80_out1.add(conv2d81_out1);
        let sigmoid67_out1 = burn::tensor::activation::sigmoid(add34_out1.clone());
        let mul81_out1 = add34_out1.mul(sigmoid67_out1);
        let conv2d82_out1 = self.conv2d82.forward(mul81_out1);
        let sigmoid68_out1 = burn::tensor::activation::sigmoid(conv2d82_out1.clone());
        let mul82_out1 = conv2d82_out1.mul(sigmoid68_out1);
        let add35_out1 = add33_out1.add(mul82_out1);
        let conv2d83_out1 = self.conv2d83.forward(slice14_out1.clone());
        let sigmoid69_out1 = burn::tensor::activation::sigmoid(conv2d83_out1.clone());
        let mul83_out1 = conv2d83_out1.mul(sigmoid69_out1);
        let concat13_out1 = burn::tensor::Tensor::cat(
            [add35_out1, mul83_out1].into(),
            1,
        );
        let conv2d84_out1 = self.conv2d84.forward(concat13_out1);
        let sigmoid70_out1 = burn::tensor::activation::sigmoid(conv2d84_out1.clone());
        let mul84_out1 = conv2d84_out1.mul(sigmoid70_out1);
        let conv2d85_out1 = self.conv2d85.forward(mul84_out1);
        let sigmoid71_out1 = burn::tensor::activation::sigmoid(conv2d85_out1.clone());
        let mul85_out1 = conv2d85_out1.mul(sigmoid71_out1);
        let conv2d86_out1 = self.conv2d86.forward(mul85_out1.clone());
        let sigmoid72_out1 = burn::tensor::activation::sigmoid(conv2d86_out1.clone());
        let mul86_out1 = conv2d86_out1.mul(sigmoid72_out1);
        let conv2d87_out1 = self.conv2d87.forward(mul86_out1.clone());
        let conv2d88_out1 = self.conv2d88.forward(mul86_out1.clone());
        let add36_out1 = conv2d87_out1.add(conv2d88_out1);
        let sigmoid73_out1 = burn::tensor::activation::sigmoid(add36_out1.clone());
        let mul87_out1 = add36_out1.mul(sigmoid73_out1);
        let conv2d89_out1 = self.conv2d89.forward(mul87_out1);
        let sigmoid74_out1 = burn::tensor::activation::sigmoid(conv2d89_out1.clone());
        let mul88_out1 = conv2d89_out1.mul(sigmoid74_out1);
        let add37_out1 = mul86_out1.add(mul88_out1);
        let conv2d90_out1 = self.conv2d90.forward(add37_out1.clone());
        let conv2d91_out1 = self.conv2d91.forward(add37_out1.clone());
        let add38_out1 = conv2d90_out1.add(conv2d91_out1);
        let sigmoid75_out1 = burn::tensor::activation::sigmoid(add38_out1.clone());
        let mul89_out1 = add38_out1.mul(sigmoid75_out1);
        let conv2d92_out1 = self.conv2d92.forward(mul89_out1);
        let sigmoid76_out1 = burn::tensor::activation::sigmoid(conv2d92_out1.clone());
        let mul90_out1 = conv2d92_out1.mul(sigmoid76_out1);
        let add39_out1 = add37_out1.add(mul90_out1);
        let conv2d93_out1 = self.conv2d93.forward(mul85_out1.clone());
        let sigmoid77_out1 = burn::tensor::activation::sigmoid(conv2d93_out1.clone());
        let mul91_out1 = conv2d93_out1.mul(sigmoid77_out1);
        let concat14_out1 = burn::tensor::Tensor::cat(
            [add39_out1, mul91_out1].into(),
            1,
        );
        let conv2d94_out1 = self.conv2d94.forward(concat14_out1);
        let sigmoid78_out1 = burn::tensor::activation::sigmoid(conv2d94_out1.clone());
        let mul92_out1 = conv2d94_out1.mul(sigmoid78_out1);
        let conv2d95_out1 = self.conv2d95.forward(mul92_out1);
        let sigmoid79_out1 = burn::tensor::activation::sigmoid(conv2d95_out1.clone());
        let mul93_out1 = conv2d95_out1.mul(sigmoid79_out1);
        let concat15_out1 = burn::tensor::Tensor::cat(
            [slice13_out1, slice14_out1, mul85_out1, mul93_out1].into(),
            1,
        );
        (concat15_out1, mul70_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule5 {
    conv2d96: Conv2d,
    conv2d97: Conv2d,
    conv2d98: Conv2d,
    conv2d99: Conv2d,
    conv2d100: Conv2d,
    conv2d101: Conv2d,
    conv2d102: Conv2d,
    resize1: burn::nn::interpolate::Interpolate2d,
    conv2d103: Conv2d,
    conv2d104: Conv2d,
    conv2d105: Conv2d,
    conv2d106: Conv2d,
    conv2d107: Conv2d,
    conv2d108: Conv2d,
    conv2d109: Conv2d,
    conv2d110: Conv2d,
    conv2d111: Conv2d,
    conv2d112: Conv2d,
    conv2d113: Conv2d,
    conv2d114: Conv2d,
    conv2d115: Conv2d,
    conv2d116: Conv2d,
    conv2d117: Conv2d,
    conv2d118: Conv2d,
    conv2d119: Conv2d,
    conv2d120: Conv2d,
    conv2d121: Conv2d,
    conv2d122: Conv2d,
    conv2d123: Conv2d,
    conv2d124: Conv2d,
    conv2d125: Conv2d,
    averagepool2d4: AvgPool2d,
    conv2d126: Conv2d,
    maxpool2d4: MaxPool2d,
    conv2d127: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule5 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d96 = Conv2dConfig::new([1024, 1024], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d97 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d98 = Conv2dConfig::new([256, 192], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d99 = Conv2dConfig::new([512, 448], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d100 = Conv2dConfig::new([1024, 960], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d101 = Conv2dConfig::new([1024, 1984], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d102 = Conv2dConfig::new([3, 64], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let resize1 = burn::nn::interpolate::Interpolate2dConfig::new()
            .with_output_size(Some([320, 320]))
            .with_scale_factor(None)
            .with_mode(burn::nn::interpolate::InterpolateMode::Nearest)
            .with_align_corners(false)
            .init();
        let conv2d103 = Conv2dConfig::new([64, 128], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d104 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d105 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d106 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d107 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d108 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d109 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d110 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d111 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d112 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d113 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d114 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d115 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d116 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d117 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d118 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d119 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d120 = Conv2dConfig::new([32, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d121 = Conv2dConfig::new([32, 32], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d122 = Conv2dConfig::new([64, 32], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d123 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d124 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d125 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d4 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d126 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d4 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d127 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d96,
            conv2d97,
            conv2d98,
            conv2d99,
            conv2d100,
            conv2d101,
            conv2d102,
            resize1,
            conv2d103,
            conv2d104,
            conv2d105,
            conv2d106,
            conv2d107,
            conv2d108,
            conv2d109,
            conv2d110,
            conv2d111,
            conv2d112,
            conv2d113,
            conv2d114,
            conv2d115,
            conv2d116,
            conv2d117,
            conv2d118,
            conv2d119,
            conv2d120,
            conv2d121,
            conv2d122,
            conv2d123,
            conv2d124,
            conv2d125,
            averagepool2d4,
            conv2d126,
            maxpool2d4,
            conv2d127,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat15_out1: Tensor<4>,
        mul1_out1: Tensor<4>,
        mul22_out1: Tensor<4>,
        mul46_out1: Tensor<4>,
        mul70_out1: Tensor<4>,
        images: Tensor<4>,
    ) -> (Tensor<5>, Tensor<4>, Tensor<4>, Tensor<4>) {
        let conv2d96_out1 = self.conv2d96.forward(concat15_out1);
        let sigmoid80_out1 = burn::tensor::activation::sigmoid(conv2d96_out1.clone());
        let mul94_out1 = conv2d96_out1.mul(sigmoid80_out1);
        let conv2d97_out1 = self.conv2d97.forward(mul1_out1);
        let conv2d98_out1 = self.conv2d98.forward(mul22_out1);
        let split_tensors = conv2d98_out1.split_with_sizes([64, 128].into(), 1);
        let [split2_out1, split2_out2] = split_tensors.try_into().unwrap();
        let conv2d99_out1 = self.conv2d99.forward(mul46_out1);
        let split_tensors = conv2d99_out1.split_with_sizes([64, 128, 256].into(), 1);
        let [split3_out1, split3_out2, split3_out3] = split_tensors.try_into().unwrap();
        let conv2d100_out1 = self.conv2d100.forward(mul70_out1);
        let split_tensors = conv2d100_out1
            .split_with_sizes([64, 128, 256, 512].into(), 1);
        let [split4_out1, split4_out2, split4_out3, split4_out4] = split_tensors
            .try_into()
            .unwrap();
        let conv2d101_out1 = self.conv2d101.forward(mul94_out1);
        let split_tensors = conv2d101_out1
            .split_with_sizes([64, 128, 256, 512, 1024].into(), 1);
        let [split5_out1, split5_out2, split5_out3, split5_out4, split5_out5] = split_tensors
            .try_into()
            .unwrap();
        let conv2d102_out1 = self.conv2d102.forward(images);
        let sigmoid81_out1 = burn::tensor::activation::sigmoid(conv2d102_out1.clone());
        let mul95_out1 = conv2d102_out1.mul(sigmoid81_out1);
        let resize1_out1 = self.resize1.forward(conv2d97_out1);
        let shape9_out1: [i64; 4] = {
            let axes = &split2_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice16_out1: [i64; 2] = shape9_out1[0..2].try_into().unwrap();
        let constant676_out1: [i64; 2] = [320i64, 320i64];
        let concat17_out1: [i64; 4usize] = [&slice16_out1[..], &constant676_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize2_out1 = {
            let target_height = concat17_out1[2] as usize;
            let target_width = concat17_out1[3] as usize;
            burn::tensor::module::interpolate(
                split2_out1,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape10_out1: [i64; 4] = {
            let axes = &split3_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice17_out1: [i64; 2] = shape10_out1[0..2].try_into().unwrap();
        let constant680_out1: [i64; 2] = [320i64, 320i64];
        let concat18_out1: [i64; 4usize] = [&slice17_out1[..], &constant680_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize3_out1 = {
            let target_height = concat18_out1[2] as usize;
            let target_width = concat18_out1[3] as usize;
            burn::tensor::module::interpolate(
                split3_out1,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape11_out1: [i64; 4] = {
            let axes = &split4_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice18_out1: [i64; 2] = shape11_out1[0..2].try_into().unwrap();
        let constant684_out1: [i64; 2] = [320i64, 320i64];
        let concat19_out1: [i64; 4usize] = [&slice18_out1[..], &constant684_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize4_out1 = {
            let target_height = concat19_out1[2] as usize;
            let target_width = concat19_out1[3] as usize;
            burn::tensor::module::interpolate(
                split4_out1,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape12_out1: [i64; 4] = {
            let axes = &split5_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice19_out1: [i64; 2] = shape12_out1[0..2].try_into().unwrap();
        let constant688_out1: [i64; 2] = [320i64, 320i64];
        let concat20_out1: [i64; 4usize] = [&slice19_out1[..], &constant688_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize5_out1 = {
            let target_height = concat20_out1[2] as usize;
            let target_width = concat20_out1[3] as usize;
            burn::tensor::module::interpolate(
                split5_out1,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let unsqueeze1_out1: Tensor<5> = resize1_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze2_out1: Tensor<5> = resize2_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze3_out1: Tensor<5> = resize3_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze4_out1: Tensor<5> = resize4_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze5_out1: Tensor<5> = resize5_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze6_out1: Tensor<5> = mul95_out1.unsqueeze_dims::<5>(&[0]);
        let concat21_out1 = burn::tensor::Tensor::cat(
            [
                unsqueeze1_out1,
                unsqueeze2_out1,
                unsqueeze3_out1,
                unsqueeze4_out1,
                unsqueeze5_out1,
                unsqueeze6_out1,
            ]
                .into(),
            0,
        );
        let reducesum1_out1 = {
            concat21_out1.sum_dim(0usize).squeeze_dims::<4usize>(&[0])
        };
        let conv2d103_out1 = self.conv2d103.forward(reducesum1_out1);
        let sigmoid82_out1 = burn::tensor::activation::sigmoid(conv2d103_out1.clone());
        let mul96_out1 = conv2d103_out1.mul(sigmoid82_out1);
        let shape13_out1: [i64; 4] = {
            let axes = &split2_out2.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice20_out1: [i64; 2] = shape13_out1[0..2].try_into().unwrap();
        let constant698_out1: [i64; 2] = [160i64, 160i64];
        let concat22_out1: [i64; 4usize] = [&slice20_out1[..], &constant698_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize6_out1 = {
            let target_height = concat22_out1[2] as usize;
            let target_width = concat22_out1[3] as usize;
            burn::tensor::module::interpolate(
                split2_out2,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape14_out1: [i64; 4] = {
            let axes = &split3_out2.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice21_out1: [i64; 2] = shape14_out1[0..2].try_into().unwrap();
        let constant702_out1: [i64; 2] = [160i64, 160i64];
        let concat23_out1: [i64; 4usize] = [&slice21_out1[..], &constant702_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize7_out1 = {
            let target_height = concat23_out1[2] as usize;
            let target_width = concat23_out1[3] as usize;
            burn::tensor::module::interpolate(
                split3_out2,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape15_out1: [i64; 4] = {
            let axes = &split4_out2.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice22_out1: [i64; 2] = shape15_out1[0..2].try_into().unwrap();
        let constant706_out1: [i64; 2] = [160i64, 160i64];
        let concat24_out1: [i64; 4usize] = [&slice22_out1[..], &constant706_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize8_out1 = {
            let target_height = concat24_out1[2] as usize;
            let target_width = concat24_out1[3] as usize;
            burn::tensor::module::interpolate(
                split4_out2,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape16_out1: [i64; 4] = {
            let axes = &split5_out2.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice23_out1: [i64; 2] = shape16_out1[0..2].try_into().unwrap();
        let constant710_out1: [i64; 2] = [160i64, 160i64];
        let concat25_out1: [i64; 4usize] = [&slice23_out1[..], &constant710_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize9_out1 = {
            let target_height = concat25_out1[2] as usize;
            let target_width = concat25_out1[3] as usize;
            burn::tensor::module::interpolate(
                split5_out2,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let unsqueeze7_out1: Tensor<5> = resize6_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze8_out1: Tensor<5> = resize7_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze9_out1: Tensor<5> = resize8_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze10_out1: Tensor<5> = resize9_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze11_out1: Tensor<5> = mul96_out1.unsqueeze_dims::<5>(&[0]);
        let concat26_out1 = burn::tensor::Tensor::cat(
            [
                unsqueeze7_out1,
                unsqueeze8_out1,
                unsqueeze9_out1,
                unsqueeze10_out1,
                unsqueeze11_out1,
            ]
                .into(),
            0,
        );
        let reducesum2_out1 = {
            concat26_out1.sum_dim(0usize).squeeze_dims::<4usize>(&[0])
        };
        let conv2d104_out1 = self.conv2d104.forward(reducesum2_out1);
        let sigmoid83_out1 = burn::tensor::activation::sigmoid(conv2d104_out1.clone());
        let mul97_out1 = conv2d104_out1.mul(sigmoid83_out1);
        let shape17_out1: [i64; 4] = {
            let axes = &mul97_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant716_out1: [i64; 1] = [1i64];
        let gather8_out1: [i64; 1usize] = constant716_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape17_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape17_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant718_out1: [i64; 1] = [1i64];
        let add40_out1 = {
            let __lhs = gather8_out1;
            let __rhs = constant718_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant719_out1: [i64; 1] = [2i64];
        let div8_out1 = {
            let __lhs = add40_out1;
            let __rhs = constant719_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice24_out1 = mul97_out1.clone().slice(s![.., 0..div8_out1[0], .., ..]);
        let constant721_out1: [i64; 1] = [2i64];
        let mul99_out1 = {
            let __lhs = div8_out1;
            let __rhs = constant721_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice25_out1 = mul97_out1.slice(s![.., div8_out1[0]..mul99_out1[0], .., ..]);
        let conv2d105_out1 = self.conv2d105.forward(slice25_out1.clone());
        let sigmoid84_out1 = burn::tensor::activation::sigmoid(conv2d105_out1.clone());
        let mul100_out1 = conv2d105_out1.mul(sigmoid84_out1);
        let conv2d106_out1 = self.conv2d106.forward(mul100_out1.clone());
        let conv2d107_out1 = self.conv2d107.forward(mul100_out1.clone());
        let add41_out1 = conv2d106_out1.add(conv2d107_out1);
        let sigmoid85_out1 = burn::tensor::activation::sigmoid(add41_out1.clone());
        let mul101_out1 = add41_out1.mul(sigmoid85_out1);
        let conv2d108_out1 = self.conv2d108.forward(mul101_out1);
        let sigmoid86_out1 = burn::tensor::activation::sigmoid(conv2d108_out1.clone());
        let mul102_out1 = conv2d108_out1.mul(sigmoid86_out1);
        let add42_out1 = mul100_out1.add(mul102_out1);
        let conv2d109_out1 = self.conv2d109.forward(add42_out1.clone());
        let conv2d110_out1 = self.conv2d110.forward(add42_out1.clone());
        let add43_out1 = conv2d109_out1.add(conv2d110_out1);
        let sigmoid87_out1 = burn::tensor::activation::sigmoid(add43_out1.clone());
        let mul103_out1 = add43_out1.mul(sigmoid87_out1);
        let conv2d111_out1 = self.conv2d111.forward(mul103_out1);
        let sigmoid88_out1 = burn::tensor::activation::sigmoid(conv2d111_out1.clone());
        let mul104_out1 = conv2d111_out1.mul(sigmoid88_out1);
        let add44_out1 = add42_out1.add(mul104_out1);
        let conv2d112_out1 = self.conv2d112.forward(slice25_out1.clone());
        let sigmoid89_out1 = burn::tensor::activation::sigmoid(conv2d112_out1.clone());
        let mul105_out1 = conv2d112_out1.mul(sigmoid89_out1);
        let concat27_out1 = burn::tensor::Tensor::cat(
            [add44_out1, mul105_out1].into(),
            1,
        );
        let conv2d113_out1 = self.conv2d113.forward(concat27_out1);
        let sigmoid90_out1 = burn::tensor::activation::sigmoid(conv2d113_out1.clone());
        let mul106_out1 = conv2d113_out1.mul(sigmoid90_out1);
        let conv2d114_out1 = self.conv2d114.forward(mul106_out1);
        let sigmoid91_out1 = burn::tensor::activation::sigmoid(conv2d114_out1.clone());
        let mul107_out1 = conv2d114_out1.mul(sigmoid91_out1);
        let conv2d115_out1 = self.conv2d115.forward(mul107_out1.clone());
        let sigmoid92_out1 = burn::tensor::activation::sigmoid(conv2d115_out1.clone());
        let mul108_out1 = conv2d115_out1.mul(sigmoid92_out1);
        let conv2d116_out1 = self.conv2d116.forward(mul108_out1.clone());
        let conv2d117_out1 = self.conv2d117.forward(mul108_out1.clone());
        let add45_out1 = conv2d116_out1.add(conv2d117_out1);
        let sigmoid93_out1 = burn::tensor::activation::sigmoid(add45_out1.clone());
        let mul109_out1 = add45_out1.mul(sigmoid93_out1);
        let conv2d118_out1 = self.conv2d118.forward(mul109_out1);
        let sigmoid94_out1 = burn::tensor::activation::sigmoid(conv2d118_out1.clone());
        let mul110_out1 = conv2d118_out1.mul(sigmoid94_out1);
        let add46_out1 = mul108_out1.add(mul110_out1);
        let conv2d119_out1 = self.conv2d119.forward(add46_out1.clone());
        let conv2d120_out1 = self.conv2d120.forward(add46_out1.clone());
        let add47_out1 = conv2d119_out1.add(conv2d120_out1);
        let sigmoid95_out1 = burn::tensor::activation::sigmoid(add47_out1.clone());
        let mul111_out1 = add47_out1.mul(sigmoid95_out1);
        let conv2d121_out1 = self.conv2d121.forward(mul111_out1);
        let sigmoid96_out1 = burn::tensor::activation::sigmoid(conv2d121_out1.clone());
        let mul112_out1 = conv2d121_out1.mul(sigmoid96_out1);
        let add48_out1 = add46_out1.add(mul112_out1);
        let conv2d122_out1 = self.conv2d122.forward(mul107_out1.clone());
        let sigmoid97_out1 = burn::tensor::activation::sigmoid(conv2d122_out1.clone());
        let mul113_out1 = conv2d122_out1.mul(sigmoid97_out1);
        let concat28_out1 = burn::tensor::Tensor::cat(
            [add48_out1, mul113_out1].into(),
            1,
        );
        let conv2d123_out1 = self.conv2d123.forward(concat28_out1);
        let sigmoid98_out1 = burn::tensor::activation::sigmoid(conv2d123_out1.clone());
        let mul114_out1 = conv2d123_out1.mul(sigmoid98_out1);
        let conv2d124_out1 = self.conv2d124.forward(mul114_out1);
        let sigmoid99_out1 = burn::tensor::activation::sigmoid(conv2d124_out1.clone());
        let mul115_out1 = conv2d124_out1.mul(sigmoid99_out1);
        let concat29_out1 = burn::tensor::Tensor::cat(
            [slice24_out1, slice25_out1, mul107_out1, mul115_out1].into(),
            1,
        );
        let conv2d125_out1 = self.conv2d125.forward(concat29_out1);
        let sigmoid100_out1 = burn::tensor::activation::sigmoid(conv2d125_out1.clone());
        let mul116_out1 = conv2d125_out1.mul(sigmoid100_out1);
        let averagepool2d4_out1 = self.averagepool2d4.forward(mul116_out1);
        let shape18_out1: [i64; 4] = {
            let axes = &averagepool2d4_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant722_out1: [i64; 1] = [1i64];
        let gather9_out1: [i64; 1usize] = constant722_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape18_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape18_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant724_out1: [i64; 1] = [1i64];
        let add49_out1 = {
            let __lhs = gather9_out1;
            let __rhs = constant724_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant725_out1: [i64; 1] = [2i64];
        let div9_out1 = {
            let __lhs = add49_out1;
            let __rhs = constant725_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice26_out1 = averagepool2d4_out1
            .clone()
            .slice(s![.., 0..div9_out1[0], .., ..]);
        let constant727_out1: [i64; 1] = [2i64];
        let mul118_out1 = {
            let __lhs = div9_out1;
            let __rhs = constant727_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice27_out1 = averagepool2d4_out1
            .slice(s![.., div9_out1[0]..mul118_out1[0], .., ..]);
        let conv2d126_out1 = self.conv2d126.forward(slice26_out1);
        let sigmoid101_out1 = burn::tensor::activation::sigmoid(conv2d126_out1.clone());
        let mul119_out1 = conv2d126_out1.mul(sigmoid101_out1);
        let maxpool2d4_out1 = self.maxpool2d4.forward(slice27_out1);
        let conv2d127_out1 = self.conv2d127.forward(maxpool2d4_out1);
        let sigmoid102_out1 = burn::tensor::activation::sigmoid(conv2d127_out1.clone());
        let mul120_out1 = conv2d127_out1.mul(sigmoid102_out1);
        let concat30_out1 = burn::tensor::Tensor::cat(
            [mul119_out1, mul120_out1].into(),
            1,
        );
        let shape19_out1: [i64; 4] = {
            let axes = &split3_out3.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice28_out1: [i64; 2] = shape19_out1[0..2].try_into().unwrap();
        let constant731_out1: [i64; 2] = [80i64, 80i64];
        let concat31_out1: [i64; 4usize] = [&slice28_out1[..], &constant731_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize10_out1 = {
            let target_height = concat31_out1[2] as usize;
            let target_width = concat31_out1[3] as usize;
            burn::tensor::module::interpolate(
                split3_out3,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape20_out1: [i64; 4] = {
            let axes = &split4_out3.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice29_out1: [i64; 2] = shape20_out1[0..2].try_into().unwrap();
        let constant735_out1: [i64; 2] = [80i64, 80i64];
        let concat32_out1: [i64; 4usize] = [&slice29_out1[..], &constant735_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize11_out1 = {
            let target_height = concat32_out1[2] as usize;
            let target_width = concat32_out1[3] as usize;
            burn::tensor::module::interpolate(
                split4_out3,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape21_out1: [i64; 4] = {
            let axes = &split5_out3.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice30_out1: [i64; 2] = shape21_out1[0..2].try_into().unwrap();
        let constant739_out1: [i64; 2] = [80i64, 80i64];
        let concat33_out1: [i64; 4usize] = [&slice30_out1[..], &constant739_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize12_out1 = {
            let target_height = concat33_out1[2] as usize;
            let target_width = concat33_out1[3] as usize;
            burn::tensor::module::interpolate(
                split5_out3,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let unsqueeze12_out1: Tensor<5> = resize10_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze13_out1: Tensor<5> = resize11_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze14_out1: Tensor<5> = resize12_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze15_out1: Tensor<5> = concat30_out1.unsqueeze_dims::<5>(&[0]);
        let concat34_out1 = burn::tensor::Tensor::cat(
            [unsqueeze12_out1, unsqueeze13_out1, unsqueeze14_out1, unsqueeze15_out1]
                .into(),
            0,
        );
        (concat34_out1, split4_out4, split5_out4, split5_out5)
    }
}
#[derive(Module, Debug)]
pub struct Submodule6 {
    conv2d128: Conv2d,
    conv2d129: Conv2d,
    conv2d130: Conv2d,
    conv2d131: Conv2d,
    conv2d132: Conv2d,
    conv2d133: Conv2d,
    conv2d134: Conv2d,
    conv2d135: Conv2d,
    conv2d136: Conv2d,
    conv2d137: Conv2d,
    conv2d138: Conv2d,
    conv2d139: Conv2d,
    conv2d140: Conv2d,
    conv2d141: Conv2d,
    conv2d142: Conv2d,
    conv2d143: Conv2d,
    conv2d144: Conv2d,
    conv2d145: Conv2d,
    conv2d146: Conv2d,
    conv2d147: Conv2d,
    conv2d148: Conv2d,
    conv2d149: Conv2d,
    averagepool2d5: AvgPool2d,
    conv2d150: Conv2d,
    maxpool2d5: MaxPool2d,
    conv2d151: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule6 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d128 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d129 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d130 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d131 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d132 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d133 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d134 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d135 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d136 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d137 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d138 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d139 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d140 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d141 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d142 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d143 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d144 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d145 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d146 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d147 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d148 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d149 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d5 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d150 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d5 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d151 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d128,
            conv2d129,
            conv2d130,
            conv2d131,
            conv2d132,
            conv2d133,
            conv2d134,
            conv2d135,
            conv2d136,
            conv2d137,
            conv2d138,
            conv2d139,
            conv2d140,
            conv2d141,
            conv2d142,
            conv2d143,
            conv2d144,
            conv2d145,
            conv2d146,
            conv2d147,
            conv2d148,
            conv2d149,
            averagepool2d5,
            conv2d150,
            maxpool2d5,
            conv2d151,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat34_out1: Tensor<5>,
        split4_out4: Tensor<4>,
        split5_out4: Tensor<4>,
    ) -> (Tensor<5>, Tensor<4>) {
        let reducesum3_out1 = {
            concat34_out1.sum_dim(0usize).squeeze_dims::<4usize>(&[0])
        };
        let conv2d128_out1 = self.conv2d128.forward(reducesum3_out1);
        let sigmoid103_out1 = burn::tensor::activation::sigmoid(conv2d128_out1.clone());
        let mul121_out1 = conv2d128_out1.mul(sigmoid103_out1);
        let shape22_out1: [i64; 4] = {
            let axes = &mul121_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant744_out1: [i64; 1] = [1i64];
        let gather10_out1: [i64; 1usize] = constant744_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape22_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape22_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant746_out1: [i64; 1] = [1i64];
        let add50_out1 = {
            let __lhs = gather10_out1;
            let __rhs = constant746_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant747_out1: [i64; 1] = [2i64];
        let div10_out1 = {
            let __lhs = add50_out1;
            let __rhs = constant747_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice31_out1 = mul121_out1.clone().slice(s![.., 0..div10_out1[0], .., ..]);
        let constant749_out1: [i64; 1] = [2i64];
        let mul123_out1 = {
            let __lhs = div10_out1;
            let __rhs = constant749_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice32_out1 = mul121_out1
            .slice(s![.., div10_out1[0]..mul123_out1[0], .., ..]);
        let conv2d129_out1 = self.conv2d129.forward(slice32_out1.clone());
        let sigmoid104_out1 = burn::tensor::activation::sigmoid(conv2d129_out1.clone());
        let mul124_out1 = conv2d129_out1.mul(sigmoid104_out1);
        let conv2d130_out1 = self.conv2d130.forward(mul124_out1.clone());
        let conv2d131_out1 = self.conv2d131.forward(mul124_out1.clone());
        let add51_out1 = conv2d130_out1.add(conv2d131_out1);
        let sigmoid105_out1 = burn::tensor::activation::sigmoid(add51_out1.clone());
        let mul125_out1 = add51_out1.mul(sigmoid105_out1);
        let conv2d132_out1 = self.conv2d132.forward(mul125_out1);
        let sigmoid106_out1 = burn::tensor::activation::sigmoid(conv2d132_out1.clone());
        let mul126_out1 = conv2d132_out1.mul(sigmoid106_out1);
        let add52_out1 = mul124_out1.add(mul126_out1);
        let conv2d133_out1 = self.conv2d133.forward(add52_out1.clone());
        let conv2d134_out1 = self.conv2d134.forward(add52_out1.clone());
        let add53_out1 = conv2d133_out1.add(conv2d134_out1);
        let sigmoid107_out1 = burn::tensor::activation::sigmoid(add53_out1.clone());
        let mul127_out1 = add53_out1.mul(sigmoid107_out1);
        let conv2d135_out1 = self.conv2d135.forward(mul127_out1);
        let sigmoid108_out1 = burn::tensor::activation::sigmoid(conv2d135_out1.clone());
        let mul128_out1 = conv2d135_out1.mul(sigmoid108_out1);
        let add54_out1 = add52_out1.add(mul128_out1);
        let conv2d136_out1 = self.conv2d136.forward(slice32_out1.clone());
        let sigmoid109_out1 = burn::tensor::activation::sigmoid(conv2d136_out1.clone());
        let mul129_out1 = conv2d136_out1.mul(sigmoid109_out1);
        let concat35_out1 = burn::tensor::Tensor::cat(
            [add54_out1, mul129_out1].into(),
            1,
        );
        let conv2d137_out1 = self.conv2d137.forward(concat35_out1);
        let sigmoid110_out1 = burn::tensor::activation::sigmoid(conv2d137_out1.clone());
        let mul130_out1 = conv2d137_out1.mul(sigmoid110_out1);
        let conv2d138_out1 = self.conv2d138.forward(mul130_out1);
        let sigmoid111_out1 = burn::tensor::activation::sigmoid(conv2d138_out1.clone());
        let mul131_out1 = conv2d138_out1.mul(sigmoid111_out1);
        let conv2d139_out1 = self.conv2d139.forward(mul131_out1.clone());
        let sigmoid112_out1 = burn::tensor::activation::sigmoid(conv2d139_out1.clone());
        let mul132_out1 = conv2d139_out1.mul(sigmoid112_out1);
        let conv2d140_out1 = self.conv2d140.forward(mul132_out1.clone());
        let conv2d141_out1 = self.conv2d141.forward(mul132_out1.clone());
        let add55_out1 = conv2d140_out1.add(conv2d141_out1);
        let sigmoid113_out1 = burn::tensor::activation::sigmoid(add55_out1.clone());
        let mul133_out1 = add55_out1.mul(sigmoid113_out1);
        let conv2d142_out1 = self.conv2d142.forward(mul133_out1);
        let sigmoid114_out1 = burn::tensor::activation::sigmoid(conv2d142_out1.clone());
        let mul134_out1 = conv2d142_out1.mul(sigmoid114_out1);
        let add56_out1 = mul132_out1.add(mul134_out1);
        let conv2d143_out1 = self.conv2d143.forward(add56_out1.clone());
        let conv2d144_out1 = self.conv2d144.forward(add56_out1.clone());
        let add57_out1 = conv2d143_out1.add(conv2d144_out1);
        let sigmoid115_out1 = burn::tensor::activation::sigmoid(add57_out1.clone());
        let mul135_out1 = add57_out1.mul(sigmoid115_out1);
        let conv2d145_out1 = self.conv2d145.forward(mul135_out1);
        let sigmoid116_out1 = burn::tensor::activation::sigmoid(conv2d145_out1.clone());
        let mul136_out1 = conv2d145_out1.mul(sigmoid116_out1);
        let add58_out1 = add56_out1.add(mul136_out1);
        let conv2d146_out1 = self.conv2d146.forward(mul131_out1.clone());
        let sigmoid117_out1 = burn::tensor::activation::sigmoid(conv2d146_out1.clone());
        let mul137_out1 = conv2d146_out1.mul(sigmoid117_out1);
        let concat36_out1 = burn::tensor::Tensor::cat(
            [add58_out1, mul137_out1].into(),
            1,
        );
        let conv2d147_out1 = self.conv2d147.forward(concat36_out1);
        let sigmoid118_out1 = burn::tensor::activation::sigmoid(conv2d147_out1.clone());
        let mul138_out1 = conv2d147_out1.mul(sigmoid118_out1);
        let conv2d148_out1 = self.conv2d148.forward(mul138_out1);
        let sigmoid119_out1 = burn::tensor::activation::sigmoid(conv2d148_out1.clone());
        let mul139_out1 = conv2d148_out1.mul(sigmoid119_out1);
        let concat37_out1 = burn::tensor::Tensor::cat(
            [slice31_out1, slice32_out1, mul131_out1, mul139_out1].into(),
            1,
        );
        let conv2d149_out1 = self.conv2d149.forward(concat37_out1);
        let sigmoid120_out1 = burn::tensor::activation::sigmoid(conv2d149_out1.clone());
        let mul140_out1 = conv2d149_out1.mul(sigmoid120_out1);
        let averagepool2d5_out1 = self.averagepool2d5.forward(mul140_out1.clone());
        let shape23_out1: [i64; 4] = {
            let axes = &averagepool2d5_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant750_out1: [i64; 1] = [1i64];
        let gather11_out1: [i64; 1usize] = constant750_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape23_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape23_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant752_out1: [i64; 1] = [1i64];
        let add59_out1 = {
            let __lhs = gather11_out1;
            let __rhs = constant752_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant753_out1: [i64; 1] = [2i64];
        let div11_out1 = {
            let __lhs = add59_out1;
            let __rhs = constant753_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice33_out1 = averagepool2d5_out1
            .clone()
            .slice(s![.., 0..div11_out1[0], .., ..]);
        let constant755_out1: [i64; 1] = [2i64];
        let mul142_out1 = {
            let __lhs = div11_out1;
            let __rhs = constant755_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice34_out1 = averagepool2d5_out1
            .slice(s![.., div11_out1[0]..mul142_out1[0], .., ..]);
        let conv2d150_out1 = self.conv2d150.forward(slice33_out1);
        let sigmoid121_out1 = burn::tensor::activation::sigmoid(conv2d150_out1.clone());
        let mul143_out1 = conv2d150_out1.mul(sigmoid121_out1);
        let maxpool2d5_out1 = self.maxpool2d5.forward(slice34_out1);
        let conv2d151_out1 = self.conv2d151.forward(maxpool2d5_out1);
        let sigmoid122_out1 = burn::tensor::activation::sigmoid(conv2d151_out1.clone());
        let mul144_out1 = conv2d151_out1.mul(sigmoid122_out1);
        let concat38_out1 = burn::tensor::Tensor::cat(
            [mul143_out1, mul144_out1].into(),
            1,
        );
        let shape24_out1: [i64; 4] = {
            let axes = &split4_out4.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice35_out1: [i64; 2] = shape24_out1[0..2].try_into().unwrap();
        let constant759_out1: [i64; 2] = [40i64, 40i64];
        let concat39_out1: [i64; 4usize] = [&slice35_out1[..], &constant759_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize13_out1 = {
            let target_height = concat39_out1[2] as usize;
            let target_width = concat39_out1[3] as usize;
            burn::tensor::module::interpolate(
                split4_out4,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let shape25_out1: [i64; 4] = {
            let axes = &split5_out4.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice36_out1: [i64; 2] = shape25_out1[0..2].try_into().unwrap();
        let constant763_out1: [i64; 2] = [40i64, 40i64];
        let concat40_out1: [i64; 4usize] = [&slice36_out1[..], &constant763_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize14_out1 = {
            let target_height = concat40_out1[2] as usize;
            let target_width = concat40_out1[3] as usize;
            burn::tensor::module::interpolate(
                split5_out4,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let unsqueeze16_out1: Tensor<5> = resize13_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze17_out1: Tensor<5> = resize14_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze18_out1: Tensor<5> = concat38_out1.unsqueeze_dims::<5>(&[0]);
        let concat41_out1 = burn::tensor::Tensor::cat(
            [unsqueeze16_out1, unsqueeze17_out1, unsqueeze18_out1].into(),
            0,
        );
        (concat41_out1, mul140_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule7 {
    conv2d152: Conv2d,
    conv2d153: Conv2d,
    conv2d154: Conv2d,
    conv2d155: Conv2d,
    conv2d156: Conv2d,
    conv2d157: Conv2d,
    conv2d158: Conv2d,
    conv2d159: Conv2d,
    conv2d160: Conv2d,
    conv2d161: Conv2d,
    conv2d162: Conv2d,
    conv2d163: Conv2d,
    conv2d164: Conv2d,
    conv2d165: Conv2d,
    conv2d166: Conv2d,
    conv2d167: Conv2d,
    conv2d168: Conv2d,
    conv2d169: Conv2d,
    conv2d170: Conv2d,
    conv2d171: Conv2d,
    conv2d172: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule7 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d152 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d153 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d154 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d155 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d156 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d157 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d158 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d159 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d160 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d161 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d162 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d163 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d164 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d165 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d166 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d167 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d168 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d169 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d170 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d171 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d172 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d152,
            conv2d153,
            conv2d154,
            conv2d155,
            conv2d156,
            conv2d157,
            conv2d158,
            conv2d159,
            conv2d160,
            conv2d161,
            conv2d162,
            conv2d163,
            conv2d164,
            conv2d165,
            conv2d166,
            conv2d167,
            conv2d168,
            conv2d169,
            conv2d170,
            conv2d171,
            conv2d172,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, concat41_out1: Tensor<5>) -> Tensor<4> {
        let reducesum4_out1 = {
            concat41_out1.sum_dim(0usize).squeeze_dims::<4usize>(&[0])
        };
        let conv2d152_out1 = self.conv2d152.forward(reducesum4_out1);
        let sigmoid123_out1 = burn::tensor::activation::sigmoid(conv2d152_out1.clone());
        let mul145_out1 = conv2d152_out1.mul(sigmoid123_out1);
        let shape26_out1: [i64; 4] = {
            let axes = &mul145_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant767_out1: [i64; 1] = [1i64];
        let gather12_out1: [i64; 1usize] = constant767_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape26_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape26_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant769_out1: [i64; 1] = [1i64];
        let add60_out1 = {
            let __lhs = gather12_out1;
            let __rhs = constant769_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant770_out1: [i64; 1] = [2i64];
        let div12_out1 = {
            let __lhs = add60_out1;
            let __rhs = constant770_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice37_out1 = mul145_out1.clone().slice(s![.., 0..div12_out1[0], .., ..]);
        let constant772_out1: [i64; 1] = [2i64];
        let mul147_out1 = {
            let __lhs = div12_out1;
            let __rhs = constant772_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice38_out1 = mul145_out1
            .slice(s![.., div12_out1[0]..mul147_out1[0], .., ..]);
        let conv2d153_out1 = self.conv2d153.forward(slice38_out1.clone());
        let sigmoid124_out1 = burn::tensor::activation::sigmoid(conv2d153_out1.clone());
        let mul148_out1 = conv2d153_out1.mul(sigmoid124_out1);
        let conv2d154_out1 = self.conv2d154.forward(mul148_out1.clone());
        let conv2d155_out1 = self.conv2d155.forward(mul148_out1.clone());
        let add61_out1 = conv2d154_out1.add(conv2d155_out1);
        let sigmoid125_out1 = burn::tensor::activation::sigmoid(add61_out1.clone());
        let mul149_out1 = add61_out1.mul(sigmoid125_out1);
        let conv2d156_out1 = self.conv2d156.forward(mul149_out1);
        let sigmoid126_out1 = burn::tensor::activation::sigmoid(conv2d156_out1.clone());
        let mul150_out1 = conv2d156_out1.mul(sigmoid126_out1);
        let add62_out1 = mul148_out1.add(mul150_out1);
        let conv2d157_out1 = self.conv2d157.forward(add62_out1.clone());
        let conv2d158_out1 = self.conv2d158.forward(add62_out1.clone());
        let add63_out1 = conv2d157_out1.add(conv2d158_out1);
        let sigmoid127_out1 = burn::tensor::activation::sigmoid(add63_out1.clone());
        let mul151_out1 = add63_out1.mul(sigmoid127_out1);
        let conv2d159_out1 = self.conv2d159.forward(mul151_out1);
        let sigmoid128_out1 = burn::tensor::activation::sigmoid(conv2d159_out1.clone());
        let mul152_out1 = conv2d159_out1.mul(sigmoid128_out1);
        let add64_out1 = add62_out1.add(mul152_out1);
        let conv2d160_out1 = self.conv2d160.forward(slice38_out1.clone());
        let sigmoid129_out1 = burn::tensor::activation::sigmoid(conv2d160_out1.clone());
        let mul153_out1 = conv2d160_out1.mul(sigmoid129_out1);
        let concat42_out1 = burn::tensor::Tensor::cat(
            [add64_out1, mul153_out1].into(),
            1,
        );
        let conv2d161_out1 = self.conv2d161.forward(concat42_out1);
        let sigmoid130_out1 = burn::tensor::activation::sigmoid(conv2d161_out1.clone());
        let mul154_out1 = conv2d161_out1.mul(sigmoid130_out1);
        let conv2d162_out1 = self.conv2d162.forward(mul154_out1);
        let sigmoid131_out1 = burn::tensor::activation::sigmoid(conv2d162_out1.clone());
        let mul155_out1 = conv2d162_out1.mul(sigmoid131_out1);
        let conv2d163_out1 = self.conv2d163.forward(mul155_out1.clone());
        let sigmoid132_out1 = burn::tensor::activation::sigmoid(conv2d163_out1.clone());
        let mul156_out1 = conv2d163_out1.mul(sigmoid132_out1);
        let conv2d164_out1 = self.conv2d164.forward(mul156_out1.clone());
        let conv2d165_out1 = self.conv2d165.forward(mul156_out1.clone());
        let add65_out1 = conv2d164_out1.add(conv2d165_out1);
        let sigmoid133_out1 = burn::tensor::activation::sigmoid(add65_out1.clone());
        let mul157_out1 = add65_out1.mul(sigmoid133_out1);
        let conv2d166_out1 = self.conv2d166.forward(mul157_out1);
        let sigmoid134_out1 = burn::tensor::activation::sigmoid(conv2d166_out1.clone());
        let mul158_out1 = conv2d166_out1.mul(sigmoid134_out1);
        let add66_out1 = mul156_out1.add(mul158_out1);
        let conv2d167_out1 = self.conv2d167.forward(add66_out1.clone());
        let conv2d168_out1 = self.conv2d168.forward(add66_out1.clone());
        let add67_out1 = conv2d167_out1.add(conv2d168_out1);
        let sigmoid135_out1 = burn::tensor::activation::sigmoid(add67_out1.clone());
        let mul159_out1 = add67_out1.mul(sigmoid135_out1);
        let conv2d169_out1 = self.conv2d169.forward(mul159_out1);
        let sigmoid136_out1 = burn::tensor::activation::sigmoid(conv2d169_out1.clone());
        let mul160_out1 = conv2d169_out1.mul(sigmoid136_out1);
        let add68_out1 = add66_out1.add(mul160_out1);
        let conv2d170_out1 = self.conv2d170.forward(mul155_out1.clone());
        let sigmoid137_out1 = burn::tensor::activation::sigmoid(conv2d170_out1.clone());
        let mul161_out1 = conv2d170_out1.mul(sigmoid137_out1);
        let concat43_out1 = burn::tensor::Tensor::cat(
            [add68_out1, mul161_out1].into(),
            1,
        );
        let conv2d171_out1 = self.conv2d171.forward(concat43_out1);
        let sigmoid138_out1 = burn::tensor::activation::sigmoid(conv2d171_out1.clone());
        let mul162_out1 = conv2d171_out1.mul(sigmoid138_out1);
        let conv2d172_out1 = self.conv2d172.forward(mul162_out1);
        let sigmoid139_out1 = burn::tensor::activation::sigmoid(conv2d172_out1.clone());
        let mul163_out1 = conv2d172_out1.mul(sigmoid139_out1);
        let concat44_out1 = burn::tensor::Tensor::cat(
            [slice37_out1, slice38_out1, mul155_out1, mul163_out1].into(),
            1,
        );
        concat44_out1
    }
}
#[derive(Module, Debug)]
pub struct Submodule8 {
    conv2d173: Conv2d,
    averagepool2d6: AvgPool2d,
    conv2d174: Conv2d,
    maxpool2d6: MaxPool2d,
    conv2d175: Conv2d,
    conv2d176: Conv2d,
    conv2d177: Conv2d,
    conv2d178: Conv2d,
    conv2d179: Conv2d,
    conv2d180: Conv2d,
    conv2d181: Conv2d,
    conv2d182: Conv2d,
    conv2d183: Conv2d,
    conv2d184: Conv2d,
    conv2d185: Conv2d,
    conv2d186: Conv2d,
    conv2d187: Conv2d,
    conv2d188: Conv2d,
    conv2d189: Conv2d,
    conv2d190: Conv2d,
    conv2d191: Conv2d,
    conv2d192: Conv2d,
    conv2d193: Conv2d,
    conv2d194: Conv2d,
    conv2d195: Conv2d,
    conv2d196: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule8 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d173 = Conv2dConfig::new([1024, 1024], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d6 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d174 = Conv2dConfig::new([512, 512], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d6 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d175 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d176 = Conv2dConfig::new([1024, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d177 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d178 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d179 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d180 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d181 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d182 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d183 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d184 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d185 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d186 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d187 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d188 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d189 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d190 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d191 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d192 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d193 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d194 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d195 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d196 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d173,
            averagepool2d6,
            conv2d174,
            maxpool2d6,
            conv2d175,
            conv2d176,
            conv2d177,
            conv2d178,
            conv2d179,
            conv2d180,
            conv2d181,
            conv2d182,
            conv2d183,
            conv2d184,
            conv2d185,
            conv2d186,
            conv2d187,
            conv2d188,
            conv2d189,
            conv2d190,
            conv2d191,
            conv2d192,
            conv2d193,
            conv2d194,
            conv2d195,
            conv2d196,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat44_out1: Tensor<4>,
        split5_out5: Tensor<4>,
    ) -> (Tensor<4>, Tensor<4>) {
        let conv2d173_out1 = self.conv2d173.forward(concat44_out1);
        let sigmoid140_out1 = burn::tensor::activation::sigmoid(conv2d173_out1.clone());
        let mul164_out1 = conv2d173_out1.mul(sigmoid140_out1);
        let averagepool2d6_out1 = self.averagepool2d6.forward(mul164_out1.clone());
        let shape27_out1: [i64; 4] = {
            let axes = &averagepool2d6_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant773_out1: [i64; 1] = [1i64];
        let gather13_out1: [i64; 1usize] = constant773_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape27_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape27_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant775_out1: [i64; 1] = [1i64];
        let add69_out1 = {
            let __lhs = gather13_out1;
            let __rhs = constant775_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant776_out1: [i64; 1] = [2i64];
        let div13_out1 = {
            let __lhs = add69_out1;
            let __rhs = constant776_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice39_out1 = averagepool2d6_out1
            .clone()
            .slice(s![.., 0..div13_out1[0], .., ..]);
        let constant778_out1: [i64; 1] = [2i64];
        let mul166_out1 = {
            let __lhs = div13_out1;
            let __rhs = constant778_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice40_out1 = averagepool2d6_out1
            .slice(s![.., div13_out1[0]..mul166_out1[0], .., ..]);
        let conv2d174_out1 = self.conv2d174.forward(slice39_out1);
        let sigmoid141_out1 = burn::tensor::activation::sigmoid(conv2d174_out1.clone());
        let mul167_out1 = conv2d174_out1.mul(sigmoid141_out1);
        let maxpool2d6_out1 = self.maxpool2d6.forward(slice40_out1);
        let conv2d175_out1 = self.conv2d175.forward(maxpool2d6_out1);
        let sigmoid142_out1 = burn::tensor::activation::sigmoid(conv2d175_out1.clone());
        let mul168_out1 = conv2d175_out1.mul(sigmoid142_out1);
        let concat45_out1 = burn::tensor::Tensor::cat(
            [mul167_out1, mul168_out1].into(),
            1,
        );
        let shape28_out1: [i64; 4] = {
            let axes = &split5_out5.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let slice41_out1: [i64; 2] = shape28_out1[0..2].try_into().unwrap();
        let constant782_out1: [i64; 2] = [20i64, 20i64];
        let concat46_out1: [i64; 4usize] = [&slice41_out1[..], &constant782_out1[..]]
            .concat()
            .try_into()
            .unwrap();
        let resize15_out1 = {
            let target_height = concat46_out1[2] as usize;
            let target_width = concat46_out1[3] as usize;
            burn::tensor::module::interpolate(
                split5_out5,
                burn::tensor::ops::InterpolateOptions::new(
                        burn::tensor::ops::InterpolateMode::Nearest,
                    )
                    .with_output_size([target_height, target_width])
                    .with_align_corners(false),
            )
        };
        let unsqueeze19_out1: Tensor<5> = resize15_out1.unsqueeze_dims::<5>(&[0]);
        let unsqueeze20_out1: Tensor<5> = concat45_out1.unsqueeze_dims::<5>(&[0]);
        let concat47_out1 = burn::tensor::Tensor::cat(
            [unsqueeze19_out1, unsqueeze20_out1].into(),
            0,
        );
        let reducesum5_out1 = {
            concat47_out1.sum_dim(0usize).squeeze_dims::<4usize>(&[0])
        };
        let conv2d176_out1 = self.conv2d176.forward(reducesum5_out1);
        let sigmoid143_out1 = burn::tensor::activation::sigmoid(conv2d176_out1.clone());
        let mul169_out1 = conv2d176_out1.mul(sigmoid143_out1);
        let shape29_out1: [i64; 4] = {
            let axes = &mul169_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant785_out1: [i64; 1] = [1i64];
        let gather14_out1: [i64; 1usize] = constant785_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape29_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape29_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant787_out1: [i64; 1] = [1i64];
        let add70_out1 = {
            let __lhs = gather14_out1;
            let __rhs = constant787_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant788_out1: [i64; 1] = [2i64];
        let div14_out1 = {
            let __lhs = add70_out1;
            let __rhs = constant788_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice42_out1 = mul169_out1.clone().slice(s![.., 0..div14_out1[0], .., ..]);
        let constant790_out1: [i64; 1] = [2i64];
        let mul171_out1 = {
            let __lhs = div14_out1;
            let __rhs = constant790_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice43_out1 = mul169_out1
            .slice(s![.., div14_out1[0]..mul171_out1[0], .., ..]);
        let conv2d177_out1 = self.conv2d177.forward(slice43_out1.clone());
        let sigmoid144_out1 = burn::tensor::activation::sigmoid(conv2d177_out1.clone());
        let mul172_out1 = conv2d177_out1.mul(sigmoid144_out1);
        let conv2d178_out1 = self.conv2d178.forward(mul172_out1.clone());
        let conv2d179_out1 = self.conv2d179.forward(mul172_out1.clone());
        let add71_out1 = conv2d178_out1.add(conv2d179_out1);
        let sigmoid145_out1 = burn::tensor::activation::sigmoid(add71_out1.clone());
        let mul173_out1 = add71_out1.mul(sigmoid145_out1);
        let conv2d180_out1 = self.conv2d180.forward(mul173_out1);
        let sigmoid146_out1 = burn::tensor::activation::sigmoid(conv2d180_out1.clone());
        let mul174_out1 = conv2d180_out1.mul(sigmoid146_out1);
        let add72_out1 = mul172_out1.add(mul174_out1);
        let conv2d181_out1 = self.conv2d181.forward(add72_out1.clone());
        let conv2d182_out1 = self.conv2d182.forward(add72_out1.clone());
        let add73_out1 = conv2d181_out1.add(conv2d182_out1);
        let sigmoid147_out1 = burn::tensor::activation::sigmoid(add73_out1.clone());
        let mul175_out1 = add73_out1.mul(sigmoid147_out1);
        let conv2d183_out1 = self.conv2d183.forward(mul175_out1);
        let sigmoid148_out1 = burn::tensor::activation::sigmoid(conv2d183_out1.clone());
        let mul176_out1 = conv2d183_out1.mul(sigmoid148_out1);
        let add74_out1 = add72_out1.add(mul176_out1);
        let conv2d184_out1 = self.conv2d184.forward(slice43_out1.clone());
        let sigmoid149_out1 = burn::tensor::activation::sigmoid(conv2d184_out1.clone());
        let mul177_out1 = conv2d184_out1.mul(sigmoid149_out1);
        let concat48_out1 = burn::tensor::Tensor::cat(
            [add74_out1, mul177_out1].into(),
            1,
        );
        let conv2d185_out1 = self.conv2d185.forward(concat48_out1);
        let sigmoid150_out1 = burn::tensor::activation::sigmoid(conv2d185_out1.clone());
        let mul178_out1 = conv2d185_out1.mul(sigmoid150_out1);
        let conv2d186_out1 = self.conv2d186.forward(mul178_out1);
        let sigmoid151_out1 = burn::tensor::activation::sigmoid(conv2d186_out1.clone());
        let mul179_out1 = conv2d186_out1.mul(sigmoid151_out1);
        let conv2d187_out1 = self.conv2d187.forward(mul179_out1.clone());
        let sigmoid152_out1 = burn::tensor::activation::sigmoid(conv2d187_out1.clone());
        let mul180_out1 = conv2d187_out1.mul(sigmoid152_out1);
        let conv2d188_out1 = self.conv2d188.forward(mul180_out1.clone());
        let conv2d189_out1 = self.conv2d189.forward(mul180_out1.clone());
        let add75_out1 = conv2d188_out1.add(conv2d189_out1);
        let sigmoid153_out1 = burn::tensor::activation::sigmoid(add75_out1.clone());
        let mul181_out1 = add75_out1.mul(sigmoid153_out1);
        let conv2d190_out1 = self.conv2d190.forward(mul181_out1);
        let sigmoid154_out1 = burn::tensor::activation::sigmoid(conv2d190_out1.clone());
        let mul182_out1 = conv2d190_out1.mul(sigmoid154_out1);
        let add76_out1 = mul180_out1.add(mul182_out1);
        let conv2d191_out1 = self.conv2d191.forward(add76_out1.clone());
        let conv2d192_out1 = self.conv2d192.forward(add76_out1.clone());
        let add77_out1 = conv2d191_out1.add(conv2d192_out1);
        let sigmoid155_out1 = burn::tensor::activation::sigmoid(add77_out1.clone());
        let mul183_out1 = add77_out1.mul(sigmoid155_out1);
        let conv2d193_out1 = self.conv2d193.forward(mul183_out1);
        let sigmoid156_out1 = burn::tensor::activation::sigmoid(conv2d193_out1.clone());
        let mul184_out1 = conv2d193_out1.mul(sigmoid156_out1);
        let add78_out1 = add76_out1.add(mul184_out1);
        let conv2d194_out1 = self.conv2d194.forward(mul179_out1.clone());
        let sigmoid157_out1 = burn::tensor::activation::sigmoid(conv2d194_out1.clone());
        let mul185_out1 = conv2d194_out1.mul(sigmoid157_out1);
        let concat49_out1 = burn::tensor::Tensor::cat(
            [add78_out1, mul185_out1].into(),
            1,
        );
        let conv2d195_out1 = self.conv2d195.forward(concat49_out1);
        let sigmoid158_out1 = burn::tensor::activation::sigmoid(conv2d195_out1.clone());
        let mul186_out1 = conv2d195_out1.mul(sigmoid158_out1);
        let conv2d196_out1 = self.conv2d196.forward(mul186_out1);
        let sigmoid159_out1 = burn::tensor::activation::sigmoid(conv2d196_out1.clone());
        let mul187_out1 = conv2d196_out1.mul(sigmoid159_out1);
        let concat50_out1 = burn::tensor::Tensor::cat(
            [slice42_out1, slice43_out1, mul179_out1, mul187_out1].into(),
            1,
        );
        (concat50_out1, mul164_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule9 {
    conv2d197: Conv2d,
    conv2d198: Conv2d,
    maxpool2d7: MaxPool2d,
    maxpool2d8: MaxPool2d,
    maxpool2d9: MaxPool2d,
    conv2d199: Conv2d,
    resize16: burn::nn::interpolate::Interpolate2d,
    conv2d200: Conv2d,
    conv2d201: Conv2d,
    conv2d202: Conv2d,
    conv2d203: Conv2d,
    conv2d204: Conv2d,
    conv2d205: Conv2d,
    conv2d206: Conv2d,
    conv2d207: Conv2d,
    conv2d208: Conv2d,
    conv2d209: Conv2d,
    conv2d210: Conv2d,
    conv2d211: Conv2d,
    conv2d212: Conv2d,
    conv2d213: Conv2d,
    conv2d214: Conv2d,
    conv2d215: Conv2d,
    conv2d216: Conv2d,
    conv2d217: Conv2d,
    conv2d218: Conv2d,
    conv2d219: Conv2d,
    conv2d220: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule9 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d197 = Conv2dConfig::new([1024, 1024], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d198 = Conv2dConfig::new([1024, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d7 = MaxPool2dConfig::new([5, 5])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Explicit(2, 2, 2, 2))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let maxpool2d8 = MaxPool2dConfig::new([5, 5])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Explicit(2, 2, 2, 2))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let maxpool2d9 = MaxPool2dConfig::new([5, 5])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Explicit(2, 2, 2, 2))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d199 = Conv2dConfig::new([1024, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let resize16 = burn::nn::interpolate::Interpolate2dConfig::new()
            .with_output_size(None)
            .with_scale_factor(Some([2.0, 2.0]))
            .with_mode(burn::nn::interpolate::InterpolateMode::Nearest)
            .with_align_corners(false)
            .init();
        let conv2d200 = Conv2dConfig::new([1536, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d201 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d202 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d203 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d204 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d205 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d206 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d207 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d208 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d209 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d210 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d211 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d212 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d213 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d214 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d215 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d216 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d217 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d218 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d219 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d220 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d197,
            conv2d198,
            maxpool2d7,
            maxpool2d8,
            maxpool2d9,
            conv2d199,
            resize16,
            conv2d200,
            conv2d201,
            conv2d202,
            conv2d203,
            conv2d204,
            conv2d205,
            conv2d206,
            conv2d207,
            conv2d208,
            conv2d209,
            conv2d210,
            conv2d211,
            conv2d212,
            conv2d213,
            conv2d214,
            conv2d215,
            conv2d216,
            conv2d217,
            conv2d218,
            conv2d219,
            conv2d220,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat50_out1: Tensor<4>,
        mul164_out1: Tensor<4>,
    ) -> (Tensor<4>, Tensor<4>) {
        let conv2d197_out1 = self.conv2d197.forward(concat50_out1);
        let sigmoid160_out1 = burn::tensor::activation::sigmoid(conv2d197_out1.clone());
        let mul188_out1 = conv2d197_out1.mul(sigmoid160_out1);
        let conv2d198_out1 = self.conv2d198.forward(mul188_out1);
        let sigmoid161_out1 = burn::tensor::activation::sigmoid(conv2d198_out1.clone());
        let mul189_out1 = conv2d198_out1.mul(sigmoid161_out1);
        let maxpool2d7_out1 = self.maxpool2d7.forward(mul189_out1.clone());
        let maxpool2d8_out1 = self.maxpool2d8.forward(maxpool2d7_out1.clone());
        let maxpool2d9_out1 = self.maxpool2d9.forward(maxpool2d8_out1.clone());
        let concat51_out1 = burn::tensor::Tensor::cat(
            [mul189_out1, maxpool2d7_out1, maxpool2d8_out1, maxpool2d9_out1].into(),
            1,
        );
        let conv2d199_out1 = self.conv2d199.forward(concat51_out1);
        let sigmoid162_out1 = burn::tensor::activation::sigmoid(conv2d199_out1.clone());
        let mul190_out1 = conv2d199_out1.mul(sigmoid162_out1);
        let resize16_out1 = self.resize16.forward(mul190_out1.clone());
        let concat52_out1 = burn::tensor::Tensor::cat(
            [resize16_out1, mul164_out1].into(),
            1,
        );
        let conv2d200_out1 = self.conv2d200.forward(concat52_out1);
        let sigmoid163_out1 = burn::tensor::activation::sigmoid(conv2d200_out1.clone());
        let mul191_out1 = conv2d200_out1.mul(sigmoid163_out1);
        let shape30_out1: [i64; 4] = {
            let axes = &mul191_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant792_out1: [i64; 1] = [1i64];
        let gather15_out1: [i64; 1usize] = constant792_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape30_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape30_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant794_out1: [i64; 1] = [1i64];
        let add79_out1 = {
            let __lhs = gather15_out1;
            let __rhs = constant794_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant795_out1: [i64; 1] = [2i64];
        let div15_out1 = {
            let __lhs = add79_out1;
            let __rhs = constant795_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice44_out1 = mul191_out1.clone().slice(s![.., 0..div15_out1[0], .., ..]);
        let constant797_out1: [i64; 1] = [2i64];
        let mul193_out1 = {
            let __lhs = div15_out1;
            let __rhs = constant797_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice45_out1 = mul191_out1
            .slice(s![.., div15_out1[0]..mul193_out1[0], .., ..]);
        let conv2d201_out1 = self.conv2d201.forward(slice45_out1.clone());
        let sigmoid164_out1 = burn::tensor::activation::sigmoid(conv2d201_out1.clone());
        let mul194_out1 = conv2d201_out1.mul(sigmoid164_out1);
        let conv2d202_out1 = self.conv2d202.forward(mul194_out1.clone());
        let conv2d203_out1 = self.conv2d203.forward(mul194_out1.clone());
        let add80_out1 = conv2d202_out1.add(conv2d203_out1);
        let sigmoid165_out1 = burn::tensor::activation::sigmoid(add80_out1.clone());
        let mul195_out1 = add80_out1.mul(sigmoid165_out1);
        let conv2d204_out1 = self.conv2d204.forward(mul195_out1);
        let sigmoid166_out1 = burn::tensor::activation::sigmoid(conv2d204_out1.clone());
        let mul196_out1 = conv2d204_out1.mul(sigmoid166_out1);
        let add81_out1 = mul194_out1.add(mul196_out1);
        let conv2d205_out1 = self.conv2d205.forward(add81_out1.clone());
        let conv2d206_out1 = self.conv2d206.forward(add81_out1.clone());
        let add82_out1 = conv2d205_out1.add(conv2d206_out1);
        let sigmoid167_out1 = burn::tensor::activation::sigmoid(add82_out1.clone());
        let mul197_out1 = add82_out1.mul(sigmoid167_out1);
        let conv2d207_out1 = self.conv2d207.forward(mul197_out1);
        let sigmoid168_out1 = burn::tensor::activation::sigmoid(conv2d207_out1.clone());
        let mul198_out1 = conv2d207_out1.mul(sigmoid168_out1);
        let add83_out1 = add81_out1.add(mul198_out1);
        let conv2d208_out1 = self.conv2d208.forward(slice45_out1.clone());
        let sigmoid169_out1 = burn::tensor::activation::sigmoid(conv2d208_out1.clone());
        let mul199_out1 = conv2d208_out1.mul(sigmoid169_out1);
        let concat53_out1 = burn::tensor::Tensor::cat(
            [add83_out1, mul199_out1].into(),
            1,
        );
        let conv2d209_out1 = self.conv2d209.forward(concat53_out1);
        let sigmoid170_out1 = burn::tensor::activation::sigmoid(conv2d209_out1.clone());
        let mul200_out1 = conv2d209_out1.mul(sigmoid170_out1);
        let conv2d210_out1 = self.conv2d210.forward(mul200_out1);
        let sigmoid171_out1 = burn::tensor::activation::sigmoid(conv2d210_out1.clone());
        let mul201_out1 = conv2d210_out1.mul(sigmoid171_out1);
        let conv2d211_out1 = self.conv2d211.forward(mul201_out1.clone());
        let sigmoid172_out1 = burn::tensor::activation::sigmoid(conv2d211_out1.clone());
        let mul202_out1 = conv2d211_out1.mul(sigmoid172_out1);
        let conv2d212_out1 = self.conv2d212.forward(mul202_out1.clone());
        let conv2d213_out1 = self.conv2d213.forward(mul202_out1.clone());
        let add84_out1 = conv2d212_out1.add(conv2d213_out1);
        let sigmoid173_out1 = burn::tensor::activation::sigmoid(add84_out1.clone());
        let mul203_out1 = add84_out1.mul(sigmoid173_out1);
        let conv2d214_out1 = self.conv2d214.forward(mul203_out1);
        let sigmoid174_out1 = burn::tensor::activation::sigmoid(conv2d214_out1.clone());
        let mul204_out1 = conv2d214_out1.mul(sigmoid174_out1);
        let add85_out1 = mul202_out1.add(mul204_out1);
        let conv2d215_out1 = self.conv2d215.forward(add85_out1.clone());
        let conv2d216_out1 = self.conv2d216.forward(add85_out1.clone());
        let add86_out1 = conv2d215_out1.add(conv2d216_out1);
        let sigmoid175_out1 = burn::tensor::activation::sigmoid(add86_out1.clone());
        let mul205_out1 = add86_out1.mul(sigmoid175_out1);
        let conv2d217_out1 = self.conv2d217.forward(mul205_out1);
        let sigmoid176_out1 = burn::tensor::activation::sigmoid(conv2d217_out1.clone());
        let mul206_out1 = conv2d217_out1.mul(sigmoid176_out1);
        let add87_out1 = add85_out1.add(mul206_out1);
        let conv2d218_out1 = self.conv2d218.forward(mul201_out1.clone());
        let sigmoid177_out1 = burn::tensor::activation::sigmoid(conv2d218_out1.clone());
        let mul207_out1 = conv2d218_out1.mul(sigmoid177_out1);
        let concat54_out1 = burn::tensor::Tensor::cat(
            [add87_out1, mul207_out1].into(),
            1,
        );
        let conv2d219_out1 = self.conv2d219.forward(concat54_out1);
        let sigmoid178_out1 = burn::tensor::activation::sigmoid(conv2d219_out1.clone());
        let mul208_out1 = conv2d219_out1.mul(sigmoid178_out1);
        let conv2d220_out1 = self.conv2d220.forward(mul208_out1);
        let sigmoid179_out1 = burn::tensor::activation::sigmoid(conv2d220_out1.clone());
        let mul209_out1 = conv2d220_out1.mul(sigmoid179_out1);
        let concat55_out1 = burn::tensor::Tensor::cat(
            [slice44_out1, slice45_out1, mul201_out1, mul209_out1].into(),
            1,
        );
        (concat55_out1, mul190_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule10 {
    conv2d221: Conv2d,
    resize17: burn::nn::interpolate::Interpolate2d,
    conv2d222: Conv2d,
    conv2d223: Conv2d,
    conv2d224: Conv2d,
    conv2d225: Conv2d,
    conv2d226: Conv2d,
    conv2d227: Conv2d,
    conv2d228: Conv2d,
    conv2d229: Conv2d,
    conv2d230: Conv2d,
    conv2d231: Conv2d,
    conv2d232: Conv2d,
    conv2d233: Conv2d,
    conv2d234: Conv2d,
    conv2d235: Conv2d,
    conv2d236: Conv2d,
    conv2d237: Conv2d,
    conv2d238: Conv2d,
    conv2d239: Conv2d,
    conv2d240: Conv2d,
    conv2d241: Conv2d,
    conv2d242: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule10 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d221 = Conv2dConfig::new([1024, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let resize17 = burn::nn::interpolate::Interpolate2dConfig::new()
            .with_output_size(None)
            .with_scale_factor(Some([2.0, 2.0]))
            .with_mode(burn::nn::interpolate::InterpolateMode::Nearest)
            .with_align_corners(false)
            .init();
        let conv2d222 = Conv2dConfig::new([1024, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d223 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d224 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d225 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d226 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d227 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d228 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d229 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d230 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d231 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d232 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d233 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d234 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d235 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d236 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d237 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d238 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d239 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d240 = Conv2dConfig::new([128, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d241 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d242 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d221,
            resize17,
            conv2d222,
            conv2d223,
            conv2d224,
            conv2d225,
            conv2d226,
            conv2d227,
            conv2d228,
            conv2d229,
            conv2d230,
            conv2d231,
            conv2d232,
            conv2d233,
            conv2d234,
            conv2d235,
            conv2d236,
            conv2d237,
            conv2d238,
            conv2d239,
            conv2d240,
            conv2d241,
            conv2d242,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat55_out1: Tensor<4>,
        mul140_out1: Tensor<4>,
    ) -> (Tensor<4>, Tensor<4>) {
        let conv2d221_out1 = self.conv2d221.forward(concat55_out1);
        let sigmoid180_out1 = burn::tensor::activation::sigmoid(conv2d221_out1.clone());
        let mul210_out1 = conv2d221_out1.mul(sigmoid180_out1);
        let resize17_out1 = self.resize17.forward(mul210_out1.clone());
        let concat56_out1 = burn::tensor::Tensor::cat(
            [resize17_out1, mul140_out1].into(),
            1,
        );
        let conv2d222_out1 = self.conv2d222.forward(concat56_out1);
        let sigmoid181_out1 = burn::tensor::activation::sigmoid(conv2d222_out1.clone());
        let mul211_out1 = conv2d222_out1.mul(sigmoid181_out1);
        let shape31_out1: [i64; 4] = {
            let axes = &mul211_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant799_out1: [i64; 1] = [1i64];
        let gather16_out1: [i64; 1usize] = constant799_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape31_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape31_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant801_out1: [i64; 1] = [1i64];
        let add88_out1 = {
            let __lhs = gather16_out1;
            let __rhs = constant801_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant802_out1: [i64; 1] = [2i64];
        let div16_out1 = {
            let __lhs = add88_out1;
            let __rhs = constant802_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice46_out1 = mul211_out1.clone().slice(s![.., 0..div16_out1[0], .., ..]);
        let constant804_out1: [i64; 1] = [2i64];
        let mul213_out1 = {
            let __lhs = div16_out1;
            let __rhs = constant804_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice47_out1 = mul211_out1
            .slice(s![.., div16_out1[0]..mul213_out1[0], .., ..]);
        let conv2d223_out1 = self.conv2d223.forward(slice47_out1.clone());
        let sigmoid182_out1 = burn::tensor::activation::sigmoid(conv2d223_out1.clone());
        let mul214_out1 = conv2d223_out1.mul(sigmoid182_out1);
        let conv2d224_out1 = self.conv2d224.forward(mul214_out1.clone());
        let conv2d225_out1 = self.conv2d225.forward(mul214_out1.clone());
        let add89_out1 = conv2d224_out1.add(conv2d225_out1);
        let sigmoid183_out1 = burn::tensor::activation::sigmoid(add89_out1.clone());
        let mul215_out1 = add89_out1.mul(sigmoid183_out1);
        let conv2d226_out1 = self.conv2d226.forward(mul215_out1);
        let sigmoid184_out1 = burn::tensor::activation::sigmoid(conv2d226_out1.clone());
        let mul216_out1 = conv2d226_out1.mul(sigmoid184_out1);
        let add90_out1 = mul214_out1.add(mul216_out1);
        let conv2d227_out1 = self.conv2d227.forward(add90_out1.clone());
        let conv2d228_out1 = self.conv2d228.forward(add90_out1.clone());
        let add91_out1 = conv2d227_out1.add(conv2d228_out1);
        let sigmoid185_out1 = burn::tensor::activation::sigmoid(add91_out1.clone());
        let mul217_out1 = add91_out1.mul(sigmoid185_out1);
        let conv2d229_out1 = self.conv2d229.forward(mul217_out1);
        let sigmoid186_out1 = burn::tensor::activation::sigmoid(conv2d229_out1.clone());
        let mul218_out1 = conv2d229_out1.mul(sigmoid186_out1);
        let add92_out1 = add90_out1.add(mul218_out1);
        let conv2d230_out1 = self.conv2d230.forward(slice47_out1.clone());
        let sigmoid187_out1 = burn::tensor::activation::sigmoid(conv2d230_out1.clone());
        let mul219_out1 = conv2d230_out1.mul(sigmoid187_out1);
        let concat57_out1 = burn::tensor::Tensor::cat(
            [add92_out1, mul219_out1].into(),
            1,
        );
        let conv2d231_out1 = self.conv2d231.forward(concat57_out1);
        let sigmoid188_out1 = burn::tensor::activation::sigmoid(conv2d231_out1.clone());
        let mul220_out1 = conv2d231_out1.mul(sigmoid188_out1);
        let conv2d232_out1 = self.conv2d232.forward(mul220_out1);
        let sigmoid189_out1 = burn::tensor::activation::sigmoid(conv2d232_out1.clone());
        let mul221_out1 = conv2d232_out1.mul(sigmoid189_out1);
        let conv2d233_out1 = self.conv2d233.forward(mul221_out1.clone());
        let sigmoid190_out1 = burn::tensor::activation::sigmoid(conv2d233_out1.clone());
        let mul222_out1 = conv2d233_out1.mul(sigmoid190_out1);
        let conv2d234_out1 = self.conv2d234.forward(mul222_out1.clone());
        let conv2d235_out1 = self.conv2d235.forward(mul222_out1.clone());
        let add93_out1 = conv2d234_out1.add(conv2d235_out1);
        let sigmoid191_out1 = burn::tensor::activation::sigmoid(add93_out1.clone());
        let mul223_out1 = add93_out1.mul(sigmoid191_out1);
        let conv2d236_out1 = self.conv2d236.forward(mul223_out1);
        let sigmoid192_out1 = burn::tensor::activation::sigmoid(conv2d236_out1.clone());
        let mul224_out1 = conv2d236_out1.mul(sigmoid192_out1);
        let add94_out1 = mul222_out1.add(mul224_out1);
        let conv2d237_out1 = self.conv2d237.forward(add94_out1.clone());
        let conv2d238_out1 = self.conv2d238.forward(add94_out1.clone());
        let add95_out1 = conv2d237_out1.add(conv2d238_out1);
        let sigmoid193_out1 = burn::tensor::activation::sigmoid(add95_out1.clone());
        let mul225_out1 = add95_out1.mul(sigmoid193_out1);
        let conv2d239_out1 = self.conv2d239.forward(mul225_out1);
        let sigmoid194_out1 = burn::tensor::activation::sigmoid(conv2d239_out1.clone());
        let mul226_out1 = conv2d239_out1.mul(sigmoid194_out1);
        let add96_out1 = add94_out1.add(mul226_out1);
        let conv2d240_out1 = self.conv2d240.forward(mul221_out1.clone());
        let sigmoid195_out1 = burn::tensor::activation::sigmoid(conv2d240_out1.clone());
        let mul227_out1 = conv2d240_out1.mul(sigmoid195_out1);
        let concat58_out1 = burn::tensor::Tensor::cat(
            [add96_out1, mul227_out1].into(),
            1,
        );
        let conv2d241_out1 = self.conv2d241.forward(concat58_out1);
        let sigmoid196_out1 = burn::tensor::activation::sigmoid(conv2d241_out1.clone());
        let mul228_out1 = conv2d241_out1.mul(sigmoid196_out1);
        let conv2d242_out1 = self.conv2d242.forward(mul228_out1);
        let sigmoid197_out1 = burn::tensor::activation::sigmoid(conv2d242_out1.clone());
        let mul229_out1 = conv2d242_out1.mul(sigmoid197_out1);
        let concat59_out1 = burn::tensor::Tensor::cat(
            [slice46_out1, slice47_out1, mul221_out1, mul229_out1].into(),
            1,
        );
        (concat59_out1, mul210_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule11 {
    conv2d243: Conv2d,
    averagepool2d7: AvgPool2d,
    conv2d244: Conv2d,
    maxpool2d10: MaxPool2d,
    conv2d245: Conv2d,
    conv2d246: Conv2d,
    conv2d247: Conv2d,
    conv2d248: Conv2d,
    conv2d249: Conv2d,
    conv2d250: Conv2d,
    conv2d251: Conv2d,
    conv2d252: Conv2d,
    conv2d253: Conv2d,
    conv2d254: Conv2d,
    conv2d255: Conv2d,
    conv2d256: Conv2d,
    conv2d257: Conv2d,
    conv2d258: Conv2d,
    conv2d259: Conv2d,
    conv2d260: Conv2d,
    conv2d261: Conv2d,
    conv2d262: Conv2d,
    conv2d263: Conv2d,
    conv2d264: Conv2d,
    conv2d265: Conv2d,
    conv2d266: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule11 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d243 = Conv2dConfig::new([512, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d7 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d244 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d10 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d245 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d246 = Conv2dConfig::new([768, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d247 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d248 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d249 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d250 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d251 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d252 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d253 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d254 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d255 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d256 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d257 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d258 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d259 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d260 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d261 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d262 = Conv2dConfig::new([128, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d263 = Conv2dConfig::new([128, 128], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d264 = Conv2dConfig::new([256, 128], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d265 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d266 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d243,
            averagepool2d7,
            conv2d244,
            maxpool2d10,
            conv2d245,
            conv2d246,
            conv2d247,
            conv2d248,
            conv2d249,
            conv2d250,
            conv2d251,
            conv2d252,
            conv2d253,
            conv2d254,
            conv2d255,
            conv2d256,
            conv2d257,
            conv2d258,
            conv2d259,
            conv2d260,
            conv2d261,
            conv2d262,
            conv2d263,
            conv2d264,
            conv2d265,
            conv2d266,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat59_out1: Tensor<4>,
        mul210_out1: Tensor<4>,
    ) -> (Tensor<4>, Tensor<4>) {
        let conv2d243_out1 = self.conv2d243.forward(concat59_out1);
        let sigmoid198_out1 = burn::tensor::activation::sigmoid(conv2d243_out1.clone());
        let mul230_out1 = conv2d243_out1.mul(sigmoid198_out1);
        let averagepool2d7_out1 = self.averagepool2d7.forward(mul230_out1.clone());
        let shape32_out1: [i64; 4] = {
            let axes = &averagepool2d7_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant805_out1: [i64; 1] = [1i64];
        let gather17_out1: [i64; 1usize] = constant805_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape32_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape32_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant807_out1: [i64; 1] = [1i64];
        let add97_out1 = {
            let __lhs = gather17_out1;
            let __rhs = constant807_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant808_out1: [i64; 1] = [2i64];
        let div17_out1 = {
            let __lhs = add97_out1;
            let __rhs = constant808_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice48_out1 = averagepool2d7_out1
            .clone()
            .slice(s![.., 0..div17_out1[0], .., ..]);
        let constant810_out1: [i64; 1] = [2i64];
        let mul232_out1 = {
            let __lhs = div17_out1;
            let __rhs = constant810_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice49_out1 = averagepool2d7_out1
            .slice(s![.., div17_out1[0]..mul232_out1[0], .., ..]);
        let conv2d244_out1 = self.conv2d244.forward(slice48_out1);
        let sigmoid199_out1 = burn::tensor::activation::sigmoid(conv2d244_out1.clone());
        let mul233_out1 = conv2d244_out1.mul(sigmoid199_out1);
        let maxpool2d10_out1 = self.maxpool2d10.forward(slice49_out1);
        let conv2d245_out1 = self.conv2d245.forward(maxpool2d10_out1);
        let sigmoid200_out1 = burn::tensor::activation::sigmoid(conv2d245_out1.clone());
        let mul234_out1 = conv2d245_out1.mul(sigmoid200_out1);
        let concat60_out1 = burn::tensor::Tensor::cat(
            [mul233_out1, mul234_out1, mul210_out1].into(),
            1,
        );
        let conv2d246_out1 = self.conv2d246.forward(concat60_out1);
        let sigmoid201_out1 = burn::tensor::activation::sigmoid(conv2d246_out1.clone());
        let mul235_out1 = conv2d246_out1.mul(sigmoid201_out1);
        let shape33_out1: [i64; 4] = {
            let axes = &mul235_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant811_out1: [i64; 1] = [1i64];
        let gather18_out1: [i64; 1usize] = constant811_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape33_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape33_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant813_out1: [i64; 1] = [1i64];
        let add98_out1 = {
            let __lhs = gather18_out1;
            let __rhs = constant813_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant814_out1: [i64; 1] = [2i64];
        let div18_out1 = {
            let __lhs = add98_out1;
            let __rhs = constant814_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice50_out1 = mul235_out1.clone().slice(s![.., 0..div18_out1[0], .., ..]);
        let constant816_out1: [i64; 1] = [2i64];
        let mul237_out1 = {
            let __lhs = div18_out1;
            let __rhs = constant816_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice51_out1 = mul235_out1
            .slice(s![.., div18_out1[0]..mul237_out1[0], .., ..]);
        let conv2d247_out1 = self.conv2d247.forward(slice51_out1.clone());
        let sigmoid202_out1 = burn::tensor::activation::sigmoid(conv2d247_out1.clone());
        let mul238_out1 = conv2d247_out1.mul(sigmoid202_out1);
        let conv2d248_out1 = self.conv2d248.forward(mul238_out1.clone());
        let conv2d249_out1 = self.conv2d249.forward(mul238_out1.clone());
        let add99_out1 = conv2d248_out1.add(conv2d249_out1);
        let sigmoid203_out1 = burn::tensor::activation::sigmoid(add99_out1.clone());
        let mul239_out1 = add99_out1.mul(sigmoid203_out1);
        let conv2d250_out1 = self.conv2d250.forward(mul239_out1);
        let sigmoid204_out1 = burn::tensor::activation::sigmoid(conv2d250_out1.clone());
        let mul240_out1 = conv2d250_out1.mul(sigmoid204_out1);
        let add100_out1 = mul238_out1.add(mul240_out1);
        let conv2d251_out1 = self.conv2d251.forward(add100_out1.clone());
        let conv2d252_out1 = self.conv2d252.forward(add100_out1.clone());
        let add101_out1 = conv2d251_out1.add(conv2d252_out1);
        let sigmoid205_out1 = burn::tensor::activation::sigmoid(add101_out1.clone());
        let mul241_out1 = add101_out1.mul(sigmoid205_out1);
        let conv2d253_out1 = self.conv2d253.forward(mul241_out1);
        let sigmoid206_out1 = burn::tensor::activation::sigmoid(conv2d253_out1.clone());
        let mul242_out1 = conv2d253_out1.mul(sigmoid206_out1);
        let add102_out1 = add100_out1.add(mul242_out1);
        let conv2d254_out1 = self.conv2d254.forward(slice51_out1.clone());
        let sigmoid207_out1 = burn::tensor::activation::sigmoid(conv2d254_out1.clone());
        let mul243_out1 = conv2d254_out1.mul(sigmoid207_out1);
        let concat61_out1 = burn::tensor::Tensor::cat(
            [add102_out1, mul243_out1].into(),
            1,
        );
        let conv2d255_out1 = self.conv2d255.forward(concat61_out1);
        let sigmoid208_out1 = burn::tensor::activation::sigmoid(conv2d255_out1.clone());
        let mul244_out1 = conv2d255_out1.mul(sigmoid208_out1);
        let conv2d256_out1 = self.conv2d256.forward(mul244_out1);
        let sigmoid209_out1 = burn::tensor::activation::sigmoid(conv2d256_out1.clone());
        let mul245_out1 = conv2d256_out1.mul(sigmoid209_out1);
        let conv2d257_out1 = self.conv2d257.forward(mul245_out1.clone());
        let sigmoid210_out1 = burn::tensor::activation::sigmoid(conv2d257_out1.clone());
        let mul246_out1 = conv2d257_out1.mul(sigmoid210_out1);
        let conv2d258_out1 = self.conv2d258.forward(mul246_out1.clone());
        let conv2d259_out1 = self.conv2d259.forward(mul246_out1.clone());
        let add103_out1 = conv2d258_out1.add(conv2d259_out1);
        let sigmoid211_out1 = burn::tensor::activation::sigmoid(add103_out1.clone());
        let mul247_out1 = add103_out1.mul(sigmoid211_out1);
        let conv2d260_out1 = self.conv2d260.forward(mul247_out1);
        let sigmoid212_out1 = burn::tensor::activation::sigmoid(conv2d260_out1.clone());
        let mul248_out1 = conv2d260_out1.mul(sigmoid212_out1);
        let add104_out1 = mul246_out1.add(mul248_out1);
        let conv2d261_out1 = self.conv2d261.forward(add104_out1.clone());
        let conv2d262_out1 = self.conv2d262.forward(add104_out1.clone());
        let add105_out1 = conv2d261_out1.add(conv2d262_out1);
        let sigmoid213_out1 = burn::tensor::activation::sigmoid(add105_out1.clone());
        let mul249_out1 = add105_out1.mul(sigmoid213_out1);
        let conv2d263_out1 = self.conv2d263.forward(mul249_out1);
        let sigmoid214_out1 = burn::tensor::activation::sigmoid(conv2d263_out1.clone());
        let mul250_out1 = conv2d263_out1.mul(sigmoid214_out1);
        let add106_out1 = add104_out1.add(mul250_out1);
        let conv2d264_out1 = self.conv2d264.forward(mul245_out1.clone());
        let sigmoid215_out1 = burn::tensor::activation::sigmoid(conv2d264_out1.clone());
        let mul251_out1 = conv2d264_out1.mul(sigmoid215_out1);
        let concat62_out1 = burn::tensor::Tensor::cat(
            [add106_out1, mul251_out1].into(),
            1,
        );
        let conv2d265_out1 = self.conv2d265.forward(concat62_out1);
        let sigmoid216_out1 = burn::tensor::activation::sigmoid(conv2d265_out1.clone());
        let mul252_out1 = conv2d265_out1.mul(sigmoid216_out1);
        let conv2d266_out1 = self.conv2d266.forward(mul252_out1);
        let sigmoid217_out1 = burn::tensor::activation::sigmoid(conv2d266_out1.clone());
        let mul253_out1 = conv2d266_out1.mul(sigmoid217_out1);
        let concat63_out1 = burn::tensor::Tensor::cat(
            [slice50_out1, slice51_out1, mul245_out1, mul253_out1].into(),
            1,
        );
        (concat63_out1, mul230_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule12 {
    conv2d267: Conv2d,
    averagepool2d8: AvgPool2d,
    conv2d268: Conv2d,
    maxpool2d11: MaxPool2d,
    conv2d269: Conv2d,
    conv2d270: Conv2d,
    conv2d271: Conv2d,
    conv2d272: Conv2d,
    conv2d273: Conv2d,
    conv2d274: Conv2d,
    conv2d275: Conv2d,
    conv2d276: Conv2d,
    conv2d277: Conv2d,
    conv2d278: Conv2d,
    conv2d279: Conv2d,
    conv2d280: Conv2d,
    conv2d281: Conv2d,
    conv2d282: Conv2d,
    conv2d283: Conv2d,
    conv2d284: Conv2d,
    conv2d285: Conv2d,
    conv2d286: Conv2d,
    conv2d287: Conv2d,
    conv2d288: Conv2d,
    conv2d289: Conv2d,
    conv2d290: Conv2d,
    #[module(skip)]
    device: Device,
}
impl Submodule12 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d267 = Conv2dConfig::new([1024, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let averagepool2d8 = AvgPool2dConfig::new([2, 2])
            .with_strides([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_count_include_pad(true)
            .with_ceil_mode(false)
            .init();
        let conv2d268 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let maxpool2d11 = MaxPool2dConfig::new([3, 3])
            .with_strides([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_ceil_mode(false)
            .init();
        let conv2d269 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d270 = Conv2dConfig::new([1024, 1024], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d271 = Conv2dConfig::new([512, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d272 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d273 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d274 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d275 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d276 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d277 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d278 = Conv2dConfig::new([512, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d279 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d280 = Conv2dConfig::new([512, 512], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d281 = Conv2dConfig::new([512, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d282 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d283 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d284 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d285 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d286 = Conv2dConfig::new([256, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d287 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d288 = Conv2dConfig::new([512, 256], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d289 = Conv2dConfig::new([512, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d290 = Conv2dConfig::new([512, 512], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        Self {
            conv2d267,
            averagepool2d8,
            conv2d268,
            maxpool2d11,
            conv2d269,
            conv2d270,
            conv2d271,
            conv2d272,
            conv2d273,
            conv2d274,
            conv2d275,
            conv2d276,
            conv2d277,
            conv2d278,
            conv2d279,
            conv2d280,
            conv2d281,
            conv2d282,
            conv2d283,
            conv2d284,
            conv2d285,
            conv2d286,
            conv2d287,
            conv2d288,
            conv2d289,
            conv2d290,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat63_out1: Tensor<4>,
        mul190_out1: Tensor<4>,
    ) -> (Tensor<4>, Tensor<4>) {
        let conv2d267_out1 = self.conv2d267.forward(concat63_out1);
        let sigmoid218_out1 = burn::tensor::activation::sigmoid(conv2d267_out1.clone());
        let mul254_out1 = conv2d267_out1.mul(sigmoid218_out1);
        let averagepool2d8_out1 = self.averagepool2d8.forward(mul254_out1.clone());
        let shape34_out1: [i64; 4] = {
            let axes = &averagepool2d8_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant817_out1: [i64; 1] = [1i64];
        let gather19_out1: [i64; 1usize] = constant817_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape34_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape34_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant819_out1: [i64; 1] = [1i64];
        let add107_out1 = {
            let __lhs = gather19_out1;
            let __rhs = constant819_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant820_out1: [i64; 1] = [2i64];
        let div19_out1 = {
            let __lhs = add107_out1;
            let __rhs = constant820_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice52_out1 = averagepool2d8_out1
            .clone()
            .slice(s![.., 0..div19_out1[0], .., ..]);
        let constant822_out1: [i64; 1] = [2i64];
        let mul256_out1 = {
            let __lhs = div19_out1;
            let __rhs = constant822_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice53_out1 = averagepool2d8_out1
            .slice(s![.., div19_out1[0]..mul256_out1[0], .., ..]);
        let conv2d268_out1 = self.conv2d268.forward(slice52_out1);
        let sigmoid219_out1 = burn::tensor::activation::sigmoid(conv2d268_out1.clone());
        let mul257_out1 = conv2d268_out1.mul(sigmoid219_out1);
        let maxpool2d11_out1 = self.maxpool2d11.forward(slice53_out1);
        let conv2d269_out1 = self.conv2d269.forward(maxpool2d11_out1);
        let sigmoid220_out1 = burn::tensor::activation::sigmoid(conv2d269_out1.clone());
        let mul258_out1 = conv2d269_out1.mul(sigmoid220_out1);
        let concat64_out1 = burn::tensor::Tensor::cat(
            [mul257_out1, mul258_out1, mul190_out1].into(),
            1,
        );
        let conv2d270_out1 = self.conv2d270.forward(concat64_out1);
        let sigmoid221_out1 = burn::tensor::activation::sigmoid(conv2d270_out1.clone());
        let mul259_out1 = conv2d270_out1.mul(sigmoid221_out1);
        let shape35_out1: [i64; 4] = {
            let axes = &mul259_out1.clone().dims()[0..4];
            let mut output = [0i64; 4];
            for i in 0..4 {
                output[i] = axes[i] as i64;
            }
            output
        };
        let constant823_out1: [i64; 1] = [1i64];
        let gather20_out1: [i64; 1usize] = constant823_out1
            .iter()
            .map(|&idx| {
                let actual_idx = if idx < 0 {
                    (shape35_out1.len() as i64 + idx) as usize
                } else {
                    idx as usize
                };
                shape35_out1[actual_idx]
            })
            .collect::<alloc::vec::Vec<_>>()
            .try_into()
            .unwrap();
        let constant825_out1: [i64; 1] = [1i64];
        let add108_out1 = {
            let __lhs = gather20_out1;
            let __rhs = constant825_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_add(__rhs[__i]))
        };
        let constant826_out1: [i64; 1] = [2i64];
        let div20_out1 = {
            let __lhs = add108_out1;
            let __rhs = constant826_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| if __rhs[__i] != 0 { __lhs[__i] / __rhs[__i] } else { __lhs[__i] })
        };
        let slice54_out1 = mul259_out1.clone().slice(s![.., 0..div20_out1[0], .., ..]);
        let constant828_out1: [i64; 1] = [2i64];
        let mul261_out1 = {
            let __lhs = div20_out1;
            let __rhs = constant828_out1;
            core::array::from_fn::<
                i64,
                1usize,
                _,
            >(|__i| (__lhs[__i]).saturating_mul(__rhs[__i]))
        };
        let slice55_out1 = mul259_out1
            .slice(s![.., div20_out1[0]..mul261_out1[0], .., ..]);
        let conv2d271_out1 = self.conv2d271.forward(slice55_out1.clone());
        let sigmoid222_out1 = burn::tensor::activation::sigmoid(conv2d271_out1.clone());
        let mul262_out1 = conv2d271_out1.mul(sigmoid222_out1);
        let conv2d272_out1 = self.conv2d272.forward(mul262_out1.clone());
        let conv2d273_out1 = self.conv2d273.forward(mul262_out1.clone());
        let add109_out1 = conv2d272_out1.add(conv2d273_out1);
        let sigmoid223_out1 = burn::tensor::activation::sigmoid(add109_out1.clone());
        let mul263_out1 = add109_out1.mul(sigmoid223_out1);
        let conv2d274_out1 = self.conv2d274.forward(mul263_out1);
        let sigmoid224_out1 = burn::tensor::activation::sigmoid(conv2d274_out1.clone());
        let mul264_out1 = conv2d274_out1.mul(sigmoid224_out1);
        let add110_out1 = mul262_out1.add(mul264_out1);
        let conv2d275_out1 = self.conv2d275.forward(add110_out1.clone());
        let conv2d276_out1 = self.conv2d276.forward(add110_out1.clone());
        let add111_out1 = conv2d275_out1.add(conv2d276_out1);
        let sigmoid225_out1 = burn::tensor::activation::sigmoid(add111_out1.clone());
        let mul265_out1 = add111_out1.mul(sigmoid225_out1);
        let conv2d277_out1 = self.conv2d277.forward(mul265_out1);
        let sigmoid226_out1 = burn::tensor::activation::sigmoid(conv2d277_out1.clone());
        let mul266_out1 = conv2d277_out1.mul(sigmoid226_out1);
        let add112_out1 = add110_out1.add(mul266_out1);
        let conv2d278_out1 = self.conv2d278.forward(slice55_out1.clone());
        let sigmoid227_out1 = burn::tensor::activation::sigmoid(conv2d278_out1.clone());
        let mul267_out1 = conv2d278_out1.mul(sigmoid227_out1);
        let concat65_out1 = burn::tensor::Tensor::cat(
            [add112_out1, mul267_out1].into(),
            1,
        );
        let conv2d279_out1 = self.conv2d279.forward(concat65_out1);
        let sigmoid228_out1 = burn::tensor::activation::sigmoid(conv2d279_out1.clone());
        let mul268_out1 = conv2d279_out1.mul(sigmoid228_out1);
        let conv2d280_out1 = self.conv2d280.forward(mul268_out1);
        let sigmoid229_out1 = burn::tensor::activation::sigmoid(conv2d280_out1.clone());
        let mul269_out1 = conv2d280_out1.mul(sigmoid229_out1);
        let conv2d281_out1 = self.conv2d281.forward(mul269_out1.clone());
        let sigmoid230_out1 = burn::tensor::activation::sigmoid(conv2d281_out1.clone());
        let mul270_out1 = conv2d281_out1.mul(sigmoid230_out1);
        let conv2d282_out1 = self.conv2d282.forward(mul270_out1.clone());
        let conv2d283_out1 = self.conv2d283.forward(mul270_out1.clone());
        let add113_out1 = conv2d282_out1.add(conv2d283_out1);
        let sigmoid231_out1 = burn::tensor::activation::sigmoid(add113_out1.clone());
        let mul271_out1 = add113_out1.mul(sigmoid231_out1);
        let conv2d284_out1 = self.conv2d284.forward(mul271_out1);
        let sigmoid232_out1 = burn::tensor::activation::sigmoid(conv2d284_out1.clone());
        let mul272_out1 = conv2d284_out1.mul(sigmoid232_out1);
        let add114_out1 = mul270_out1.add(mul272_out1);
        let conv2d285_out1 = self.conv2d285.forward(add114_out1.clone());
        let conv2d286_out1 = self.conv2d286.forward(add114_out1.clone());
        let add115_out1 = conv2d285_out1.add(conv2d286_out1);
        let sigmoid233_out1 = burn::tensor::activation::sigmoid(add115_out1.clone());
        let mul273_out1 = add115_out1.mul(sigmoid233_out1);
        let conv2d287_out1 = self.conv2d287.forward(mul273_out1);
        let sigmoid234_out1 = burn::tensor::activation::sigmoid(conv2d287_out1.clone());
        let mul274_out1 = conv2d287_out1.mul(sigmoid234_out1);
        let add116_out1 = add114_out1.add(mul274_out1);
        let conv2d288_out1 = self.conv2d288.forward(mul269_out1.clone());
        let sigmoid235_out1 = burn::tensor::activation::sigmoid(conv2d288_out1.clone());
        let mul275_out1 = conv2d288_out1.mul(sigmoid235_out1);
        let concat66_out1 = burn::tensor::Tensor::cat(
            [add116_out1, mul275_out1].into(),
            1,
        );
        let conv2d289_out1 = self.conv2d289.forward(concat66_out1);
        let sigmoid236_out1 = burn::tensor::activation::sigmoid(conv2d289_out1.clone());
        let mul276_out1 = conv2d289_out1.mul(sigmoid236_out1);
        let conv2d290_out1 = self.conv2d290.forward(mul276_out1);
        let sigmoid237_out1 = burn::tensor::activation::sigmoid(conv2d290_out1.clone());
        let mul277_out1 = conv2d290_out1.mul(sigmoid237_out1);
        let concat67_out1 = burn::tensor::Tensor::cat(
            [slice54_out1, slice55_out1, mul269_out1, mul277_out1].into(),
            1,
        );
        (concat67_out1, mul254_out1)
    }
}
#[derive(Module, Debug)]
pub struct Submodule13 {
    conv2d291: Conv2d,
    conv2d292: Conv2d,
    conv2d293: Conv2d,
    conv2d294: Conv2d,
    conv2d295: Conv2d,
    conv2d296: Conv2d,
    conv2d297: Conv2d,
    conv3d1: Conv3d,
    conv2d298: Conv2d,
    conv2d299: Conv2d,
    conv2d300: Conv2d,
    conv2d301: Conv2d,
    conv2d302: Conv2d,
    conv2d303: Conv2d,
    conv3d2: Conv3d,
    conv2d304: Conv2d,
    conv2d305: Conv2d,
    conv2d306: Conv2d,
    conv2d307: Conv2d,
    conv2d308: Conv2d,
    conv2d309: Conv2d,
    conv3d3: Conv3d,
    #[module(skip)]
    device: Device,
}
impl Submodule13 {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let conv2d291 = Conv2dConfig::new([2048, 512], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d292 = Conv2dConfig::new([256, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d293 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(4)
            .with_bias(true)
            .init(device);
        let conv2d294 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(4)
            .with_bias(true)
            .init(device);
        let conv2d295 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d296 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d297 = Conv2dConfig::new([256, 1], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv3d1 = Conv3dConfig::new([16, 1], [1, 1, 1])
            .with_stride([1, 1, 1])
            .with_padding(PaddingConfig3d::Valid)
            .with_dilation([1, 1, 1])
            .with_groups(1)
            .with_bias(false)
            .init(device);
        let conv2d298 = Conv2dConfig::new([512, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d299 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(4)
            .with_bias(true)
            .init(device);
        let conv2d300 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(4)
            .with_bias(true)
            .init(device);
        let conv2d301 = Conv2dConfig::new([512, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d302 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d303 = Conv2dConfig::new([256, 1], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv3d2 = Conv3dConfig::new([16, 1], [1, 1, 1])
            .with_stride([1, 1, 1])
            .with_padding(PaddingConfig3d::Valid)
            .with_dilation([1, 1, 1])
            .with_groups(1)
            .with_bias(false)
            .init(device);
        let conv2d304 = Conv2dConfig::new([512, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d305 = Conv2dConfig::new([64, 64], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(4)
            .with_bias(true)
            .init(device);
        let conv2d306 = Conv2dConfig::new([64, 64], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(4)
            .with_bias(true)
            .init(device);
        let conv2d307 = Conv2dConfig::new([512, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d308 = Conv2dConfig::new([256, 256], [3, 3])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv2d309 = Conv2dConfig::new([256, 1], [1, 1])
            .with_stride([1, 1])
            .with_padding(PaddingConfig2d::Valid)
            .with_dilation([1, 1])
            .with_groups(1)
            .with_bias(true)
            .init(device);
        let conv3d3 = Conv3dConfig::new([16, 1], [1, 1, 1])
            .with_stride([1, 1, 1])
            .with_padding(PaddingConfig3d::Valid)
            .with_dilation([1, 1, 1])
            .with_groups(1)
            .with_bias(false)
            .init(device);
        Self {
            conv2d291,
            conv2d292,
            conv2d293,
            conv2d294,
            conv2d295,
            conv2d296,
            conv2d297,
            conv3d1,
            conv2d298,
            conv2d299,
            conv2d300,
            conv2d301,
            conv2d302,
            conv2d303,
            conv3d2,
            conv2d304,
            conv2d305,
            conv2d306,
            conv2d307,
            conv2d308,
            conv2d309,
            conv3d3,
            device: device.clone(),
        }
    }
    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(
        &self,
        concat67_out1: Tensor<4>,
        mul230_out1: Tensor<4>,
        mul254_out1: Tensor<4>,
    ) -> Tensor<2> {
        let conv2d291_out1 = self.conv2d291.forward(concat67_out1);
        let sigmoid238_out1 = burn::tensor::activation::sigmoid(conv2d291_out1.clone());
        let mul278_out1 = conv2d291_out1.mul(sigmoid238_out1);
        let conv2d292_out1 = self.conv2d292.forward(mul230_out1.clone());
        let sigmoid239_out1 = burn::tensor::activation::sigmoid(conv2d292_out1.clone());
        let mul279_out1 = conv2d292_out1.mul(sigmoid239_out1);
        let conv2d293_out1 = self.conv2d293.forward(mul279_out1);
        let sigmoid240_out1 = burn::tensor::activation::sigmoid(conv2d293_out1.clone());
        let mul280_out1 = conv2d293_out1.mul(sigmoid240_out1);
        let conv2d294_out1 = self.conv2d294.forward(mul280_out1);
        let conv2d295_out1 = self.conv2d295.forward(mul230_out1);
        let sigmoid241_out1 = burn::tensor::activation::sigmoid(conv2d295_out1.clone());
        let mul281_out1 = conv2d295_out1.mul(sigmoid241_out1);
        let conv2d296_out1 = self.conv2d296.forward(mul281_out1);
        let sigmoid242_out1 = burn::tensor::activation::sigmoid(conv2d296_out1.clone());
        let mul282_out1 = conv2d296_out1.mul(sigmoid242_out1);
        let conv2d297_out1 = self.conv2d297.forward(mul282_out1);
        let reshape1_out1 = conv2d294_out1.reshape([1, 4, 16, 80, 80]);
        let transpose1_out1 = reshape1_out1.permute([0, 2, 1, 3, 4]);
        let softmax1_out1 = burn::tensor::activation::softmax(transpose1_out1, 1);
        let conv3d1_out1 = self.conv3d1.forward(softmax1_out1);
        let gather21_out1 = {
            let sliced = conv3d1_out1.slice(s![.., 0, .., .., ..]);
            sliced.squeeze_dim::<4usize>(1)
        };
        let conv2d298_out1 = self.conv2d298.forward(mul254_out1.clone());
        let sigmoid243_out1 = burn::tensor::activation::sigmoid(conv2d298_out1.clone());
        let mul283_out1 = conv2d298_out1.mul(sigmoid243_out1);
        let conv2d299_out1 = self.conv2d299.forward(mul283_out1);
        let sigmoid244_out1 = burn::tensor::activation::sigmoid(conv2d299_out1.clone());
        let mul284_out1 = conv2d299_out1.mul(sigmoid244_out1);
        let conv2d300_out1 = self.conv2d300.forward(mul284_out1);
        let conv2d301_out1 = self.conv2d301.forward(mul254_out1);
        let sigmoid245_out1 = burn::tensor::activation::sigmoid(conv2d301_out1.clone());
        let mul285_out1 = conv2d301_out1.mul(sigmoid245_out1);
        let conv2d302_out1 = self.conv2d302.forward(mul285_out1);
        let sigmoid246_out1 = burn::tensor::activation::sigmoid(conv2d302_out1.clone());
        let mul286_out1 = conv2d302_out1.mul(sigmoid246_out1);
        let conv2d303_out1 = self.conv2d303.forward(mul286_out1);
        let reshape2_out1 = conv2d300_out1.reshape([1, 4, 16, 40, 40]);
        let transpose2_out1 = reshape2_out1.permute([0, 2, 1, 3, 4]);
        let softmax2_out1 = burn::tensor::activation::softmax(transpose2_out1, 1);
        let conv3d2_out1 = self.conv3d2.forward(softmax2_out1);
        let gather22_out1 = {
            let sliced = conv3d2_out1.slice(s![.., 0, .., .., ..]);
            sliced.squeeze_dim::<4usize>(1)
        };
        let conv2d304_out1 = self.conv2d304.forward(mul278_out1.clone());
        let sigmoid247_out1 = burn::tensor::activation::sigmoid(conv2d304_out1.clone());
        let mul287_out1 = conv2d304_out1.mul(sigmoid247_out1);
        let conv2d305_out1 = self.conv2d305.forward(mul287_out1);
        let sigmoid248_out1 = burn::tensor::activation::sigmoid(conv2d305_out1.clone());
        let mul288_out1 = conv2d305_out1.mul(sigmoid248_out1);
        let conv2d306_out1 = self.conv2d306.forward(mul288_out1);
        let conv2d307_out1 = self.conv2d307.forward(mul278_out1);
        let sigmoid249_out1 = burn::tensor::activation::sigmoid(conv2d307_out1.clone());
        let mul289_out1 = conv2d307_out1.mul(sigmoid249_out1);
        let conv2d308_out1 = self.conv2d308.forward(mul289_out1);
        let sigmoid250_out1 = burn::tensor::activation::sigmoid(conv2d308_out1.clone());
        let mul290_out1 = conv2d308_out1.mul(sigmoid250_out1);
        let conv2d309_out1 = self.conv2d309.forward(mul290_out1);
        let reshape3_out1 = conv2d306_out1.reshape([1, 4, 16, 20, 20]);
        let transpose3_out1 = reshape3_out1.permute([0, 2, 1, 3, 4]);
        let softmax3_out1 = burn::tensor::activation::softmax(transpose3_out1, 1);
        let conv3d3_out1 = self.conv3d3.forward(softmax3_out1);
        let gather23_out1 = {
            let sliced = conv3d3_out1.slice(s![.., 0, .., .., ..]);
            sliced.squeeze_dim::<4usize>(1)
        };
        let flatten1_out1 = {
            let leading_dim = conv2d297_out1.shape()[..1].iter().product::<usize>()
                as i32;
            conv2d297_out1.reshape::<2, _>([leading_dim, -1])
        };
        let flatten2_out1 = {
            let leading_dim = gather21_out1.shape()[..1].iter().product::<usize>()
                as i32;
            gather21_out1.reshape::<2, _>([leading_dim, -1])
        };
        let flatten3_out1 = {
            let leading_dim = conv2d303_out1.shape()[..1].iter().product::<usize>()
                as i32;
            conv2d303_out1.reshape::<2, _>([leading_dim, -1])
        };
        let flatten4_out1 = {
            let leading_dim = gather22_out1.shape()[..1].iter().product::<usize>()
                as i32;
            gather22_out1.reshape::<2, _>([leading_dim, -1])
        };
        let flatten5_out1 = {
            let leading_dim = conv2d309_out1.shape()[..1].iter().product::<usize>()
                as i32;
            conv2d309_out1.reshape::<2, _>([leading_dim, -1])
        };
        let flatten6_out1 = {
            let leading_dim = gather23_out1.shape()[..1].iter().product::<usize>()
                as i32;
            gather23_out1.reshape::<2, _>([leading_dim, -1])
        };
        let concat68_out1 = burn::tensor::Tensor::cat(
            [
                flatten1_out1,
                flatten2_out1,
                flatten3_out1,
                flatten4_out1,
                flatten5_out1,
                flatten6_out1,
            ]
                .into(),
            1,
        );
        concat68_out1
    }
}

#[derive(Module, Debug)]
pub struct Model {
    submodule1: Submodule1,
    submodule2: Submodule2,
    submodule3: Submodule3,
    submodule4: Submodule4,
    submodule5: Submodule5,
    submodule6: Submodule6,
    submodule7: Submodule7,
    submodule8: Submodule8,
    submodule9: Submodule9,
    submodule10: Submodule10,
    submodule11: Submodule11,
    submodule12: Submodule12,
    submodule13: Submodule13,
    #[module(skip)]
    device: Device,
}


extern crate std;

impl Model {
    /// Load model weights from a burnpack file.
    pub fn from_file<P: AsRef<std::path::Path>>(file: P, device: &Device) -> Self {
        let mut model = Self::new(device);
        let mut store = BurnpackStore::from_file(&file);
        model
            .load_from(&mut store)
            .unwrap_or_else(|e| {
                panic!("Failed to load burnpack file {}: {e}", file.as_ref().display())
            });
        model
    }

    /// Load model weights from in-memory bytes.
    ///
    /// The bytes must be the contents of a `.bpk` file.
    pub fn from_bytes(bytes: Bytes, device: &Device) -> Self {
        let mut model = Self::new(device);
        let mut store = BurnpackStore::from_bytes(Some(bytes));
        model
            .load_from(&mut store)
            .unwrap_or_else(|e| panic!("Failed to load burnpack bytes: {e}"));
        model
    }
}

impl Model {
    #[allow(unused_variables)]
    pub fn new(device: &Device) -> Self {
        let submodule1 = Submodule1::new(device);
        let submodule2 = Submodule2::new(device);
        let submodule3 = Submodule3::new(device);
        let submodule4 = Submodule4::new(device);
        let submodule5 = Submodule5::new(device);
        let submodule6 = Submodule6::new(device);
        let submodule7 = Submodule7::new(device);
        let submodule8 = Submodule8::new(device);
        let submodule9 = Submodule9::new(device);
        let submodule10 = Submodule10::new(device);
        let submodule11 = Submodule11::new(device);
        let submodule12 = Submodule12::new(device);
        let submodule13 = Submodule13::new(device);
        Self {
            submodule1,
            submodule2,
            submodule3,
            submodule4,
            submodule5,
            submodule6,
            submodule7,
            submodule8,
            submodule9,
            submodule10,
            submodule11,
            submodule12,
            submodule13,
            device: device.clone(),
        }
    }

    #[allow(clippy::let_and_return, clippy::approx_constant)]
    pub fn forward(&self, images: Tensor<4>) -> Tensor<2> {
        let (concat3_out1, mul1_out1) = self.submodule1.forward(images.clone());
        let (concat7_out1, mul22_out1) = self.submodule2.forward(concat3_out1);
        let (concat11_out1, mul46_out1) = self.submodule3.forward(concat7_out1);
        let (concat15_out1, mul70_out1) = self.submodule4.forward(concat11_out1);
        let (concat34_out1, split4_out4, split5_out4, split5_out5) = self
            .submodule5
            .forward(
                concat15_out1,
                mul1_out1,
                mul22_out1,
                mul46_out1,
                mul70_out1,
                images,
            );
        let (concat41_out1, mul140_out1) = self
            .submodule6
            .forward(concat34_out1, split4_out4, split5_out4);
        let concat44_out1 = self.submodule7.forward(concat41_out1);
        let (concat50_out1, mul164_out1) = self
            .submodule8
            .forward(concat44_out1, split5_out5);
        let (concat55_out1, mul190_out1) = self
            .submodule9
            .forward(concat50_out1, mul164_out1);
        let (concat59_out1, mul210_out1) = self
            .submodule10
            .forward(concat55_out1, mul140_out1);
        let (concat63_out1, mul230_out1) = self
            .submodule11
            .forward(concat59_out1, mul210_out1);
        let (concat67_out1, mul254_out1) = self
            .submodule12
            .forward(concat63_out1, mul190_out1);
        let concat68_out1 = self
            .submodule13
            .forward(concat67_out1, mul230_out1, mul254_out1);
        concat68_out1
    }
}
