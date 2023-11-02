// Read camera config file
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use log::{error, info};
use serde::de::Visitor;
use serde::ser::Error;
use serde::{Deserialize, Deserializer};
use std::fs;
use std::ops::Add;
use toml::value::Offset;

fn default_constant_recorder() -> bool {
    false
}

fn default_min_disk_space_mb() -> u32 {
    200
}

fn default_location_timestamp() -> Option<u64> {
    None
}

fn default_location_accuracy() -> Option<f32> {
    None
}
fn default_output_dir() -> String {
    String::from("/var/spool/cptv")
}
fn default_activate_thermal_throttler() -> bool {
    false
}

#[derive(Debug)]
struct TimeUnit(char);

#[derive(Debug)]
struct NumberString(String, Option<TimeUnit>, bool);

fn from_time_abs_or_rel_str<'de, D>(deserializer: D) -> Result<AbsRelTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    // NOTE: This is probably not that robust on all possible input strings – but we should solve this
    //  with better validation/UI elsewhere where users are inputting time offsets
    let mut tokens: Vec<NumberString> = Vec::new();
    for char in s.chars() {
        match char {
            '-' | '+' | '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                if let Some(NumberString(ref mut n, _, _)) = tokens.last_mut() {
                    n.push(char);
                } else {
                    tokens.push(NumberString(String::from(char), None, true));
                }
            }
            's' | 'h' | 'm' | 'z' => {
                if let Some(NumberString(_, ref mut o, _)) = tokens.last_mut() {
                    *o = Some(TimeUnit(char));
                } else {
                    // Parse error
                }
                tokens.push(NumberString(String::from(""), None, true));
            }
            ':' => {
                let count = tokens.len();
                if let Some(NumberString(_, ref mut o, ref mut is_relative)) = tokens.last_mut() {
                    if count == 1 {
                        *o = Some(TimeUnit('h'));
                    } else if count == 2 {
                        *o = Some(TimeUnit('m'));
                    } else if count == 3 {
                        *o = Some(TimeUnit('s'));
                    };
                    *is_relative = false;
                } else {
                    // Parse error
                }
                tokens.push(NumberString(String::from(""), None, false));
            }
            _ => {} // Skip unknown, maybe log a parse error?
        }
    }
    let mut relative_time_seconds = None;
    let mut absolute_time = None;
    for token in &tokens {
        if token.2 {
            if relative_time_seconds.is_none() {
                relative_time_seconds = Some(0);
            }
        } else {
            if absolute_time.is_none() {
                absolute_time = Some(HourMin { hour: 0, min: 0 });
            }
        }
        if let Some(ref mut seconds) = relative_time_seconds {
            if let Ok(mut num) = i32::from_str_radix(&token.0, 10) {
                if let Some(unit) = &token.1 {
                    let mul = match unit.0 {
                        's' => 1,
                        'm' => 60,
                        'h' => 60 * 60,
                        _ => 1,
                    };
                    num *= mul;
                } else {
                    num *= 60; // Default unit is minutes if none specified
                }
                if *seconds < 0 && num > 0 {
                    *seconds += -num;
                } else {
                    *seconds += num;
                }
            }
        } else if let Some(ref mut hour_min) = absolute_time {
            if let Ok(num) = i32::from_str_radix(&token.0, 10) {
                if let Some(unit) = &token.1 {
                    match unit.0 {
                        'm' => hour_min.min = num as u8,
                        'h' => hour_min.hour = num as u8,
                        _ => {}
                    };
                }
            }
        }
    }

    Ok(AbsRelTime {
        absolute_time,
        relative_time_seconds,
    })
}

fn timestamp_to_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let date_time: toml::value::Datetime = Deserialize::deserialize(deserializer)?;
    let date = date_time.date.expect("Should have date");
    let time = date_time.time.expect("should have time");
    let offset = date_time.offset.expect("should have offset");
    let offset_minutes = match offset {
        Offset::Z => 0,
        Offset::Custom { minutes } => minutes,
    } as i32;
    let fixed_offset = if offset_minutes < 0 {
        FixedOffset::east_opt(offset_minutes * 60)
    } else {
        FixedOffset::west_opt(offset_minutes * 60)
    };
    if let Some(fixed_offset) = fixed_offset {
        let naive_utc = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(date.year as i32, date.month as u32, date.day as u32).unwrap(),
            NaiveTime::from_hms_nano_opt(
                time.hour as u32,
                time.minute as u32,
                time.second as u32,
                time.nanosecond,
            )
            .unwrap(),
        )
        .add(fixed_offset);
        let local = DateTime::<Utc>::from_naive_utc_and_offset(naive_utc, Utc);
        Ok(Some(local.with_timezone(&Utc).timestamp_micros() as u64))
    } else {
        Ok(None)
    }
}

fn location_accuracy_to_f32<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    let location_accuracy: f32 = Deserialize::deserialize(deserializer)?;
    if location_accuracy == 0.0 {
        Ok(None)
    } else {
        Ok(Some(location_accuracy))
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
struct LocationSettings {
    latitude: Option<f32>,
    longitude: Option<f32>,
    altitude: Option<f32>,

    #[serde(
        deserialize_with = "timestamp_to_u64",
        default = "default_location_timestamp"
    )]
    timestamp: Option<u64>,
    #[serde(
        deserialize_with = "location_accuracy_to_f32",
        default = "default_location_accuracy"
    )]
    accuracy: Option<f32>,
}

#[derive(Debug, PartialEq, Clone)]
struct HourMin {
    hour: u8,
    min: u8,
}
#[derive(Debug, PartialEq, Clone)]
struct AbsRelTime {
    absolute_time: Option<HourMin>,
    relative_time_seconds: Option<i32>,
}

impl AbsRelTime {
    pub fn time_offset(&self) -> Option<(bool, i32)> {
        // Absolute or relative time in seconds in the day
        if let Some(abs_time) = &self.absolute_time {
            Some((
                true,
                (abs_time.hour as i32 * 60 * 60) + (abs_time.min as i32 * 60),
            ))
        } else {
            Some((false, self.relative_time_seconds.unwrap()))
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
struct TimeWindow {
    #[serde(
        rename = "start-recording",
        deserialize_with = "from_time_abs_or_rel_str"
    )]
    start_recording: AbsRelTime,
    #[serde(
        rename = "stop-recording",
        deserialize_with = "from_time_abs_or_rel_str"
    )]
    stop_recording: AbsRelTime,
}

impl Default for TimeWindow {
    fn default() -> Self {
        TimeWindow {
            start_recording: AbsRelTime {
                absolute_time: None,
                relative_time_seconds: Some(-(30 * 60)),
            },
            stop_recording: AbsRelTime {
                absolute_time: None,
                relative_time_seconds: Some(30 * 60),
            },
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
struct DeviceRegistration {
    id: Option<u32>,
    group: Option<String>,
    name: Option<String>,
    server: Option<String>,
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
struct ThermalRecordingSettings {
    #[serde(rename = "output-dir", default = "default_output_dir")]
    output_dir: String,
    #[serde(rename = "constant-recorder", default = "default_constant_recorder")]
    constant_recorder: bool,
    #[serde(rename = "min-disk-space-mb", default = "default_min_disk_space_mb")]
    min_disk_space_mb: u32,
}

impl Default for ThermalRecordingSettings {
    fn default() -> Self {
        ThermalRecordingSettings {
            output_dir: default_output_dir(),
            constant_recorder: default_constant_recorder(),
            min_disk_space_mb: default_min_disk_space_mb(),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
struct ThermalThrottlerSettings {
    #[serde(default = "default_activate_thermal_throttler")]
    activate: bool,
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
pub struct DeviceConfig {
    #[serde(rename = "windows", default)]
    recording_window: TimeWindow,
    #[serde(rename = "device")]
    device_info: Option<DeviceRegistration>,
    #[serde(rename = "thermal-recorder", default)]
    recording_settings: ThermalRecordingSettings,
    location: Option<LocationSettings>,
}

impl DeviceConfig {
    pub fn has_location(&self) -> bool {
        if let Some(location_settings) = &self.location {
            location_settings.longitude.is_some() && location_settings.latitude.is_some()
        } else {
            false
        }
    }
    pub fn is_registered(&self) -> bool {
        if let Some(device) = &self.device_info {
            device.id.is_some() && device.name.is_some() && device.group.is_some()
        } else {
            false
        }
    }

    // Only call these once we know the device is registered
    pub fn device_id(&self) -> u32 {
        self.device_info.unwrap().id.unwrap()
    }

    pub fn device_name(&self) -> &[u8] {
        self.device_info.unwrap().name.unwrap().as_bytes()
    }

    pub fn lat_lng(&self) -> (f32, f32) {
        (
            self.location.unwrap().latitude.unwrap(),
            self.location.unwrap().longitude.unwrap(),
        )
    }
    pub fn location_timestamp(&self) -> Option<u64> {
        self.location.unwrap().timestamp
    }
    pub fn location_altitude(&self) -> Option<f32> {
        self.location.unwrap().altitude
    }
    pub fn location_accuracy(&self) -> Option<f32> {
        self.location.unwrap().accuracy
    }
    pub fn recording_window(&self) -> (AbsRelTime, AbsRelTime) {
        (
            self.recording_window.start_recording.clone(),
            self.recording_window.stop_recording.clone(),
        )
    }
}

pub fn load_device_config() -> Option<DeviceConfig> {
    let config_toml = fs::read("/etc/cacophony/config.toml").ok()?;
    let config_toml_str = String::from_utf8(config_toml).ok()?;
    let device_config: DeviceConfig = toml::from_str(&config_toml_str).ok()?;

    // TODO: Make sure device has sane windows etc.

    if !device_config.has_location() {
        error!("No location set for this device. To enter recording mode, a location must be set.");
        // TODO: Event log error?
        std::process::exit(1);
    }
    info!("Got config {:?}", device_config);

    Some(device_config)
}
fn main() {}

#[cfg(test)]
mod tests {
    use crate::device_config::DeviceConfig;

    #[test]
    fn load_config() {
        let config: DeviceConfig = toml::from_str(
            r#"
[device]
id = 1
group = "test-group"
name = "test-name"
server = "test-url"

[thermal-recorder]
use-sunrise-sunset = false
max-secs = 300
min-disk-space-mb = 200
min-secs = 5
output-dir = "/var/spool/cptv"
preview-secs = 1

[location]
accuracy = 0.0
altitude = 103.0
latitude = -46.60101
longitude = 172.71303
timestamp = 2023-11-02T08:24:21+13:00
updated = 2023-11-02T08:24:21+13:00

[thermal-throttler]
activate = true

[windows]

  start-recording = "30m"
stop-recording = "07:50"
"#,
        )
        .unwrap();
        println!("Config {:#?}", config)
    }
}
