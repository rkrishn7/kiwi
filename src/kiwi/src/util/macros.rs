macro_rules! try_conv_bail {
    ($name:expr, $s:expr) => {
        $name.try_into().expect($s)
    };
}

pub(crate) use try_conv_bail;
