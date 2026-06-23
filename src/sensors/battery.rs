use super::types::SensorReading;

use embassy_time::{Duration, Timer};
use esp_hal::analog::adc::{Adc, AdcCalLine, AdcConfig, AdcPin, Attenuation};
use esp_hal::peripherals::{ADC1, GPIO0};
use esp_hal::Blocking;



const DIVIDER_RATIO: f32 = 2.0;
const BATT_EMPTY_MV: f32 = 3300.0;
const BATT_FULL_MV: f32 = 4200.0;


pub struct Battery {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    pin: AdcPin<GPIO0<'static>, ADC1<'static>, AdcCalLine<ADC1<'static>>>,
}

impl Battery {
    pub fn new(adc1: ADC1<'static>, gpio0: GPIO0<'static>) -> Self {
        let mut config = AdcConfig::new();
        // 11dB attenuation -> ~3.3V full scale; the divided LiPo sits at 1.65-2.1V.
        let pin = config.enable_pin_with_cal::<_, AdcCalLine<ADC1<'static>>>(gpio0, Attenuation::_11dB);
        let adc = Adc::new(adc1, config);
        Self { adc, pin }
    }

    pub async fn read(&mut self) -> SensorReading {
        // calibrated read returns millivolts; poll until the conversion is done
        let adc_mv = loop {
            if let Ok(mv) = self.adc.read_oneshot(&mut self.pin) {
                break mv;
            }
            Timer::after(Duration::from_millis(2)).await;
        };
        SensorReading::Battery { percent: percent_from_adc_mv(adc_mv) }
    }
}

fn percent_from_adc_mv(adc_mv: u16) -> f32 {
    let batt_mv = adc_mv as f32 * DIVIDER_RATIO;
    ((batt_mv - BATT_EMPTY_MV) / (BATT_FULL_MV - BATT_EMPTY_MV) * 100.0).clamp(0.0, 100.0)
}
