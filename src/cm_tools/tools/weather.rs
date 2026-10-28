//! 获取当地天气（Open-Meteo API，无需 Key）

use serde::Deserialize;

use crate::cm_tools::tool_result::ToolError;

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
///
/// 失败语义（Open-Meteo 无需 Key，所以没有「未配置」分支）：
/// - 参数：缺 / 过短的 `city` / `location` → `invalid_args`；
/// - 客户端构造失败 → `weather_client_failed`；网络超时 → `timeout`；其余网络 / HTTP / 解析失败 → `weather_api_error`；
/// - **查不到该地名** → `weather_city_not_found`。旧实现返回 `未找到与「…」匹配的地点…`，
///   正文既不以「错误」开头也不含「失败 / 超时」，被 `parse_legacy_output` 判成**成功**；
/// - 上游未给 `current` → `weather_no_current_data`，旧实现同样是静默成功。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str, timeout_secs: u64) -> Result<String, ToolError> {
    let city = parse_weather_city(args_json)?;
    let client = build_blocking_http_client(timeout_secs)?;
    let loc = geocode_city_blocking(&client, &city)?;
    let forecast = forecast_blocking(&client, &loc)?;
    let cur = forecast.current.ok_or_else(no_current_weather_error)?;
    Ok(format_weather_line(&loc, &cur))
}

/// 上游响应里没有 `current` 字段：数据缺失，属外部原因。
fn no_current_weather_error() -> ToolError {
    ToolError::external_code(
        "weather_no_current_data",
        "未获取到当前天气数据".to_string(),
    )
}

/// 地名没有匹配结果：**调用方换一个城市名就能成**，故归 `invalid_args` 一侧的输入问题。
fn city_not_found_error(city: &str) -> ToolError {
    ToolError::with_code(
        "weather_city_not_found",
        format!("未找到与「{}」匹配的地点，请换一个城市或地区名重试。", city),
    )
}

#[allow(clippy::result_large_err)]
fn parse_weather_city(args_json: &str) -> Result<String, ToolError> {
    let args: super::tool_param_types::GetWeatherArgs =
        super::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let city = args
        .city
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            args.location
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        });
    match city {
        Some(s) if s.len() >= 2 => Ok(s.to_string()),
        _ => Err(ToolError::invalid_args(
            "错误：请提供 city 或 location 参数（至少 2 个字符）".to_string(),
        )),
    }
}

#[allow(clippy::result_large_err)]
fn build_blocking_http_client(timeout_secs: u64) -> Result<reqwest::blocking::Client, ToolError> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| {
            ToolError::external_code(
                "weather_client_failed",
                format!("请求客户端创建失败：{}", e),
            )
        })
}

#[allow(clippy::result_large_err)]
fn geocode_city_blocking(
    client: &reqwest::blocking::Client,
    city: &str,
) -> Result<GeoResult, ToolError> {
    let geo: GeocodingResponse = match client
        .get(GEOCODING_URL)
        .query(&[("name", city), ("count", "1"), ("language", "zh")])
        .send()
    {
        Ok(res) if res.status().is_success() => match res.json() {
            Ok(j) => j,
            Err(e) => {
                return Err(weather_api_error(format!(
                    "错误[weather_api_error]：解析地理编码结果失败：{}",
                    e
                )))
            }
        },
        Ok(res) => {
            return Err(weather_api_error(format!(
                "错误[weather_api_error]：地理编码请求失败：{}",
                res.status()
            )))
        }
        Err(e) => return Err(weather_network_error(&e)),
    };
    geo.results
        .and_then(|r| r.into_iter().next())
        .ok_or_else(|| city_not_found_error(city))
}

#[allow(clippy::result_large_err)]
fn forecast_blocking(
    client: &reqwest::blocking::Client,
    loc: &GeoResult,
) -> Result<ForecastResponse, ToolError> {
    let lat = loc.latitude.to_string();
    let lon = loc.longitude.to_string();
    match client
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
            Ok(j) => Ok(j),
            Err(e) => Err(weather_api_error(format!(
                "错误[weather_api_error]：解析天气结果失败：{}",
                e
            ))),
        },
        Ok(res) => Err(weather_api_error(format!(
            "错误[weather_api_error]：天气请求失败：{}",
            res.status()
        ))),
        Err(e) => Err(weather_network_error(&e)),
    }
}

/// Open-Meteo 的 HTTP / 解析失败（外部，不重试）。正文保留既有的 `错误[码]：` 首行标记。
fn weather_api_error(text: String) -> ToolError {
    ToolError::with_code("weather_api_error", text)
}

/// reqwest 发送失败：超时归 `timeout`（瞬时、可重试），其余网络失败归 `weather_api_error`（外部、不重试）。
/// 首行显式错误码标记仍保留，供尚未走 `Err` 的旧路径（如信封文本）识别。
fn weather_network_error(e: &reqwest::Error) -> ToolError {
    if e.is_timeout() {
        ToolError::with_code("timeout", format!("错误[timeout]：网络请求超时：{}", e))
    } else {
        weather_api_error(format!("错误[weather_api_error]：网络请求失败：{}", e))
    }
}

fn format_weather_line(loc: &GeoResult, cur: &CurrentWeather) -> String {
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
        loc.name.clone()
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
        let e = run_try("{}", 15).expect_err("缺 city 应失败");
        assert_eq!(e.code, "invalid_args");
        assert!(
            e.message.contains("city") || e.message.contains("location"),
            "缺少参数应提示，得到: {}",
            e.message
        );
    }

    #[test]
    fn test_run_city_too_short() {
        let e = run_try(r#"{"city":"x"}"#, 15).expect_err("过短 city 应失败");
        assert_eq!(e.code, "invalid_args");
        assert!(
            e.message.contains("至少 2 个字符")
                || e.message.contains("city")
                || e.message.contains("location"),
            "得到: {}",
            e.message
        );
    }

    /// 查不到地名是**显式失败**：旧实现返回 `未找到与「…」匹配的地点…`，
    /// 既不以「错误」开头也不含「失败 / 超时」，被启发式判成 `ok: true` —— 模型会以为查询成功。
    #[test]
    fn city_not_found_is_an_explicit_failure() {
        let e = city_not_found_error("Atlantis");
        assert_eq!(e.code, "weather_city_not_found");
        assert!(e.message.contains("Atlantis"), "{}", e.message);
    }

    /// 上游没给 `current` 同样是显式失败（旧实现的 `未获取到当前天气数据` 也是静默成功）。
    #[test]
    fn missing_current_section_is_an_explicit_failure() {
        let e = no_current_weather_error();
        assert_eq!(e.code, "weather_no_current_data");
    }
}
