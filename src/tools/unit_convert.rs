//! 物理量与数据量单位换算（基于 [`uom`]，不调用外部进程）。
//!
//! 支持长度、质量、温度、信息量（字节/比特及常见 SI/二进制前缀）、时间、面积、压强、速度。

use uom::si::area::{
    acre, hectare, square_centimeter, square_kilometer, square_meter, square_mile,
};
use uom::si::f64::{
    Area, Information, Length, Mass, Pressure, ThermodynamicTemperature, Time, Velocity,
};
use uom::si::information::{
    bit, byte, gibibyte, gigabyte, kibibyte, kilobyte, mebibyte, megabyte, tebibyte, terabyte,
};
use uom::si::length::{
    centimeter, foot, inch, kilometer, meter, micrometer, mile, millimeter, nanometer, yard,
};
use uom::si::mass::{gram, kilogram, milligram, ounce, pound};
use uom::si::pressure::{atmosphere, bar, pascal};
use uom::si::thermodynamic_temperature::{degree_celsius, degree_fahrenheit, kelvin};
use uom::si::time::{day, hour, microsecond, millisecond, minute, second};
use uom::si::velocity::{kilometer_per_hour, meter_per_second, mile_per_hour};

fn norm_unit(s: &str) -> String {
    s.trim().to_lowercase().replace('μ', "u")
}

/// JSON：`category`（length|mass|temperature|data|time|area|pressure|speed）、`value`（数字）、`from`、`to`（单位符号或常用别名）。
pub fn run(args_json: &str) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let category = match v.get("category").and_then(|c| c.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return "错误：缺少 category（如 length、mass、temperature、data、time、area、pressure、speed）".to_string(),
    };
    let value = match v.get("value").and_then(|x| x.as_f64()) {
        Some(x) if x.is_finite() => x,
        _ => return "错误：缺少 value 或不是有限数字".to_string(),
    };
    let from = match v.get("from").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return "错误：缺少 from（源单位）".to_string(),
    };
    let to = match v.get("to").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return "错误：缺少 to（目标单位）".to_string(),
    };

    let cat = norm_unit(category);
    let res = match cat.as_str() {
        "length" | "距离" | "长度" => convert_length(value, from, to),
        "mass" | "质量" | "重量" => convert_mass(value, from, to),
        "temperature" | "temp" | "温度" => convert_temperature(value, from, to),
        "data" | "information" | "存储" | "数据量" => convert_data(value, from, to),
        "time" | "时间" | "时长" => convert_time(value, from, to),
        "area" | "面积" => convert_area(value, from, to),
        "pressure" | "压强" | "压力" => convert_pressure(value, from, to),
        "speed" | "velocity" | "速度" => convert_speed(value, from, to),
        _ => Err(format!("不支持的 category：{category}")),
    };

    match res {
        Ok((out, from_label, to_label)) => format!(
            "换算结果：{} {} = {:.6} {}（基于 uom SI）",
            trim_trailing_zeros(value),
            from_label,
            out,
            to_label
        ),
        Err(e) => format!("错误：{e}"),
    }
}

fn trim_trailing_zeros(x: f64) -> String {
    let s = format!("{x:.12}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        format!("{x}")
    } else {
        s.to_string()
    }
}

fn convert_length(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "m" | "meter" | "meters" | "metre" | "metres" | "米" => Length::new::<meter>(value),
        "km" | "kilometer" | "kilometers" | "kilometre" | "公里" | "千米" => {
            Length::new::<kilometer>(value)
        }
        "cm" | "centimeter" | "centimeters" | "厘米" => Length::new::<centimeter>(value),
        "mm" | "millimeter" | "millimeters" | "毫米" => Length::new::<millimeter>(value),
        "um" | "micrometer" | "micrometers" | "micron" | "微米" => {
            Length::new::<micrometer>(value)
        }
        "nm" | "nanometer" | "nanometers" | "纳米" => Length::new::<nanometer>(value),
        "mi" | "mile" | "miles" | "英里" => Length::new::<mile>(value),
        "ft" | "foot" | "feet" | "英尺" => Length::new::<foot>(value),
        "in" | "inch" | "inches" | "英寸" => Length::new::<inch>(value),
        "yd" | "yard" | "yards" | "码" => Length::new::<yard>(value),
        _ => return Err(format!("未知长度单位 from：{from}")),
    };
    let out = match t.as_str() {
        "m" | "meter" | "meters" | "metre" | "metres" | "米" => q.get::<meter>(),
        "km" | "kilometer" | "kilometers" | "kilometre" | "公里" | "千米" => {
            q.get::<kilometer>()
        }
        "cm" | "centimeter" | "centimeters" | "厘米" => q.get::<centimeter>(),
        "mm" | "millimeter" | "millimeters" | "毫米" => q.get::<millimeter>(),
        "um" | "micrometer" | "micrometers" | "micron" | "微米" => q.get::<micrometer>(),
        "nm" | "nanometer" | "nanometers" | "纳米" => q.get::<nanometer>(),
        "mi" | "mile" | "miles" | "英里" => q.get::<mile>(),
        "ft" | "foot" | "feet" | "英尺" => q.get::<foot>(),
        "in" | "inch" | "inches" | "英寸" => q.get::<inch>(),
        "yd" | "yard" | "yards" | "码" => q.get::<yard>(),
        _ => return Err(format!("未知长度单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_mass(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "kg" | "kilogram" | "kilograms" | "千克" | "公斤" => Mass::new::<kilogram>(value),
        "g" | "gram" | "grams" | "克" => Mass::new::<gram>(value),
        "mg" | "milligram" | "milligrams" | "毫克" => Mass::new::<milligram>(value),
        "lb" | "lbs" | "pound" | "pounds" | "磅" => Mass::new::<pound>(value),
        "oz" | "ounce" | "ounces" | "盎司" => Mass::new::<ounce>(value),
        _ => return Err(format!("未知质量单位 from：{from}")),
    };
    let out = match t.as_str() {
        "kg" | "kilogram" | "kilograms" | "千克" | "公斤" => q.get::<kilogram>(),
        "g" | "gram" | "grams" | "克" => q.get::<gram>(),
        "mg" | "milligram" | "milligrams" | "毫克" => q.get::<milligram>(),
        "lb" | "lbs" | "pound" | "pounds" | "磅" => q.get::<pound>(),
        "oz" | "ounce" | "ounces" | "盎司" => q.get::<ounce>(),
        _ => return Err(format!("未知质量单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_temperature(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "c" | "celsius" | "degc" | "℃" | "°c" | "摄氏度" => {
            ThermodynamicTemperature::new::<degree_celsius>(value)
        }
        "f" | "fahrenheit" | "degf" | "℉" | "°f" | "华氏度" => {
            ThermodynamicTemperature::new::<degree_fahrenheit>(value)
        }
        "k" | "kelvin" | "开尔文" => ThermodynamicTemperature::new::<kelvin>(value),
        _ => {
            return Err(format!(
                "未知温度单位 from：{from}（支持 C/F/K 及 celsius/fahrenheit/kelvin）"
            ));
        }
    };
    let out = match t.as_str() {
        "c" | "celsius" | "degc" | "℃" | "°c" | "摄氏度" => q.get::<degree_celsius>(),
        "f" | "fahrenheit" | "degf" | "℉" | "°f" | "华氏度" => q.get::<degree_fahrenheit>(),
        "k" | "kelvin" | "开尔文" => q.get::<kelvin>(),
        _ => return Err(format!("未知温度单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_data(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "b" | "bit" | "bits" | "比特" => Information::new::<bit>(value),
        "byte" | "bytes" | "b8" | "字节" => Information::new::<byte>(value),
        "kb" | "kilobyte" | "kilo_byte" => Information::new::<kilobyte>(value),
        "kib" | "kibibyte" | "ki" => Information::new::<kibibyte>(value),
        "mb" | "megabyte" => Information::new::<megabyte>(value),
        "mib" | "mebibyte" | "mi" => Information::new::<mebibyte>(value),
        "gb" | "gigabyte" => Information::new::<gigabyte>(value),
        "gib" | "gibibyte" | "gi" => Information::new::<gibibyte>(value),
        "tb" | "terabyte" => Information::new::<terabyte>(value),
        "tib" | "tebibyte" | "ti" => Information::new::<tebibyte>(value),
        _ => {
            return Err(format!(
                "未知数据单位 from：{from}（支持 bit、byte、KB/MB/GB/TB 十进制与 KiB/MiB/GiB/TiB 二进制）"
            ));
        }
    };
    let out = match t.as_str() {
        "b" | "bit" | "bits" | "比特" => q.get::<bit>(),
        "byte" | "bytes" | "b8" | "字节" => q.get::<byte>(),
        "kb" | "kilobyte" | "kilo_byte" => q.get::<kilobyte>(),
        "kib" | "kibibyte" | "ki" => q.get::<kibibyte>(),
        "mb" | "megabyte" => q.get::<megabyte>(),
        "mib" | "mebibyte" | "mi" => q.get::<mebibyte>(),
        "gb" | "gigabyte" => q.get::<gigabyte>(),
        "gib" | "gibibyte" | "gi" => q.get::<gibibyte>(),
        "tb" | "terabyte" => q.get::<terabyte>(),
        "tib" | "tebibyte" | "ti" => q.get::<tebibyte>(),
        _ => return Err(format!("未知数据单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_time(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "s" | "sec" | "second" | "seconds" | "秒" => Time::new::<second>(value),
        "ms" | "millisecond" | "milliseconds" | "毫秒" => Time::new::<millisecond>(value),
        "us" | "microsecond" | "microseconds" | "微秒" => Time::new::<microsecond>(value),
        "min" | "minute" | "minutes" | "分" | "分钟" => Time::new::<minute>(value),
        "h" | "hr" | "hour" | "hours" | "时" | "小时" => Time::new::<hour>(value),
        "d" | "day" | "days" | "天" | "日" => Time::new::<day>(value),
        _ => return Err(format!("未知时间单位 from：{from}")),
    };
    let out = match t.as_str() {
        "s" | "sec" | "second" | "seconds" | "秒" => q.get::<second>(),
        "ms" | "millisecond" | "milliseconds" | "毫秒" => q.get::<millisecond>(),
        "us" | "microsecond" | "microseconds" | "微秒" => q.get::<microsecond>(),
        "min" | "minute" | "minutes" | "分" | "分钟" => q.get::<minute>(),
        "h" | "hr" | "hour" | "hours" | "时" | "小时" => q.get::<hour>(),
        "d" | "day" | "days" | "天" | "日" => q.get::<day>(),
        _ => return Err(format!("未知时间单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_area(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "m2" | "sqm" | "m^2" | "square_meter" | "square_meters" | "平方米" => {
            Area::new::<square_meter>(value)
        }
        "km2" | "sqkm" | "km^2" | "square_kilometer" | "平方千米" => {
            Area::new::<square_kilometer>(value)
        }
        "cm2" | "sqcm" | "cm^2" | "square_centimeter" | "平方厘米" => {
            Area::new::<square_centimeter>(value)
        }
        "mi2" | "sqmi" | "square_mile" | "平方英里" => Area::new::<square_mile>(value),
        "acre" | "acres" | "英亩" => Area::new::<acre>(value),
        "ha" | "hectare" | "hectares" | "公顷" => Area::new::<hectare>(value),
        _ => return Err(format!("未知面积单位 from：{from}")),
    };
    let out = match t.as_str() {
        "m2" | "sqm" | "m^2" | "square_meter" | "square_meters" | "平方米" => {
            q.get::<square_meter>()
        }
        "km2" | "sqkm" | "km^2" | "square_kilometer" | "平方千米" => {
            q.get::<square_kilometer>()
        }
        "cm2" | "sqcm" | "cm^2" | "square_centimeter" | "平方厘米" => {
            q.get::<square_centimeter>()
        }
        "mi2" | "sqmi" | "square_mile" | "平方英里" => q.get::<square_mile>(),
        "acre" | "acres" | "英亩" => q.get::<acre>(),
        "ha" | "hectare" | "hectares" | "公顷" => q.get::<hectare>(),
        _ => return Err(format!("未知面积单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_pressure(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "pa" | "pascal" | "pascals" | "帕" | "帕斯卡" => Pressure::new::<pascal>(value),
        "bar" | "bars" => Pressure::new::<bar>(value),
        "atm" | "atmosphere" | "atmospheres" | "大气压" => Pressure::new::<atmosphere>(value),
        _ => return Err(format!("未知压强单位 from：{from}（支持 Pa、bar、atm）")),
    };
    let out = match t.as_str() {
        "pa" | "pascal" | "pascals" | "帕" | "帕斯卡" => q.get::<pascal>(),
        "bar" | "bars" => q.get::<bar>(),
        "atm" | "atmosphere" | "atmospheres" | "大气压" => q.get::<atmosphere>(),
        _ => return Err(format!("未知压强单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

fn convert_speed(value: f64, from: &str, to: &str) -> Result<(f64, String, String), String> {
    let f = norm_unit(from);
    let t = norm_unit(to);
    let q = match f.as_str() {
        "mps" | "m/s" | "meter_per_second" | "米每秒" => {
            Velocity::new::<meter_per_second>(value)
        }
        "kmh" | "km/h" | "kph" | "kilometer_per_hour" | "千米每小时" => {
            Velocity::new::<kilometer_per_hour>(value)
        }
        "mph" | "mi/h" | "mile_per_hour" | "英里每小时" => {
            Velocity::new::<mile_per_hour>(value)
        }
        _ => return Err(format!("未知速度单位 from：{from}（支持 m/s、km/h、mph）")),
    };
    let out = match t.as_str() {
        "mps" | "m/s" | "meter_per_second" | "米每秒" => q.get::<meter_per_second>(),
        "kmh" | "km/h" | "kph" | "kilometer_per_hour" | "千米每小时" => {
            q.get::<kilometer_per_hour>()
        }
        "mph" | "mi/h" | "mile_per_hour" | "英里每小时" => q.get::<mile_per_hour>(),
        _ => return Err(format!("未知速度单位 to：{to}")),
    };
    Ok((out, from.to_string(), to.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn km_to_miles() {
        let s = run(r#"{"category":"length","value":1,"from":"km","to":"mile"}"#);
        assert!(s.contains("换算结果"), "{s}");
        assert!(s.contains("0.621371") || s.contains("0.62137"), "{s}");
    }

    #[test]
    fn c_to_f() {
        let s = run(r#"{"category":"temperature","value":100,"from":"C","to":"F"}"#);
        assert!(s.contains("212"), "{s}");
    }

    #[test]
    fn gib_to_mb_decimal() {
        let s = run(r#"{"category":"data","value":1,"from":"GiB","to":"MB"}"#);
        assert!(s.contains("换算结果"), "{s}");
    }
}
