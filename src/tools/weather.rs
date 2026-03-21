//! 获取当地天气（Open-Meteo API，无需 Key）

use serde::Deserialize;

const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeoResult>>,
}

#[derive(Debug, Deserialize)]
struct GeoResult {
    name: String,
    latitude: f64,
    longitude: f64,
    timezone: String,
    #[serde(default)]
    country: String,
}

#[derive(Debug, Deserialize)]
struct ForecastResponse {
    current: Option<CurrentWeather>,
}

#[derive(Debug, Deserialize)]
struct CurrentWeather {
    temperature_2m: f64,
    relative_humidity_2m: Option<f64>,
    weather_code: Option<f64>,
    wind_speed_10m: Option<f64>,
}

/// WMO 天气现象代码简要描述（0–99）
fn weather_code_text(code: f64) -> &'static str {
    match code as u32 {
        0 => "晴",
        1 => "大部晴朗",
        2 => "少云",
        3 => "多云",
        45 => "雾",
        48 => "冻雾",
        51..=57 => "毛毛雨",
        61..=67 => "雨",
        71..=77 => "雪",
        80..=82 => "阵雨",
        85..=86 => "阵雪",
        95 => "雷暴",
        96..=99 => "雷暴伴冰雹",
        _ => "未知",
    }
}

/// 根据城市名（或地区名）获取当前天气，返回格式化的简短描述。`timeout_secs` 为 HTTP 请求超时（秒）。
pub fn run(args_json: &str, timeout_secs: u64) -> String {
    let city = match serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|v| {
            v.get("city")
                .or(v.get("location"))
                .and_then(|c| c.as_str())
                .map(String::from)
        }) {
        Some(s) if s.len() >= 2 => s.trim().to_string(),
        _ => return "错误：请提供 city 或 location 参数（至少 2 个字符）".to_string(),
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("请求客户端创建失败：{}", e),
    };

    let geo: GeocodingResponse = match client
        .get(GEOCODING_URL)
        .query(&[("name", city.as_str()), ("count", "1"), ("language", "zh")])
        .send()
    {
        Ok(res) if res.status().is_success() => match res.json() {
            Ok(j) => j,
            Err(e) => return format!("解析地理编码结果失败：{}", e),
        },
        Ok(res) => return format!("地理编码请求失败：{}", res.status()),
        Err(e) => return format!("网络请求失败：{}", e),
    };

    let loc = match geo.results.and_then(|r| r.into_iter().next()) {
        Some(l) => l,
        None => return format!("未找到与「{}」匹配的地点，请换一个城市或地区名重试。", city),
    };

    let lat = loc.latitude.to_string();
    let lon = loc.longitude.to_string();
    let forecast: ForecastResponse = match client
        .get(FORECAST_URL)
        .query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            (
                "current",
                "temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m",
            ),
            ("timezone", loc.timezone.as_str()),
        ])
        .send()
    {
        Ok(res) if res.status().is_success() => match res.json() {
            Ok(j) => j,
            Err(e) => return format!("解析天气结果失败：{}", e),
        },
        Ok(res) => return format!("天气请求失败：{}", res.status()),
        Err(e) => return format!("网络请求失败：{}", e),
    };

    let cur = match forecast.current {
        Some(c) => c,
        None => return "未获取到当前天气数据".to_string(),
    };

    let desc = cur.weather_code.map(weather_code_text).unwrap_or("—");
    let hum = cur
        .relative_humidity_2m
        .map(|h| format!("湿度 {}%", h as i32))
        .unwrap_or_default();
    let wind = cur
        .wind_speed_10m
        .map(|w| format!("风速 {} km/h", w as i32))
        .unwrap_or_default();
    let extra = [hum, wind]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("，");

    let location_name = if loc.country.is_empty() {
        loc.name
    } else {
        format!("{}（{}）", loc.name, loc.country)
    };
    format!(
        "{}：{}，气温 {}°C{}{}",
        location_name,
        desc,
        cur.temperature_2m as i32,
        if extra.is_empty() { "" } else { "，" },
        extra
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_missing_city() {
        let out = run("{}", 15);
        assert!(
            out.contains("city") || out.contains("location"),
            "缺少参数应提示，得到: {}",
            out
        );
    }

    #[test]
    fn test_run_city_too_short() {
        let out = run(r#"{"city":"x"}"#, 15);
        assert!(
            out.contains("至少 2 个字符") || out.contains("city") || out.contains("location"),
            "得到: {}",
            out
        );
    }
}
