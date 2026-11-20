/** The name of the product behind each driver, as shown beside a device. */
export const BRAND: Record<string, string> = {
  airzone: "Airzone",
  shelly: "Shelly",
  hoymiles: "Hoymiles",
  trydan: "V2C Trydan",
  hue: "Philips Hue",
  homekit: "HomeKit",
  renson: "Renson",
  netatmo: "Legrand Écocompteur",
  ezviz: "EZVIZ",
  esphome: "ESPHome",
  tasmota: "Tasmota",
  tplink: "TP-Link",
  reolink: "Reolink",
  forecast_solar: "Forecast.Solar",
}

export const brand = (driver: string) => BRAND[driver] ?? driver
