// Generated from server/contracts/views.json by `BREEZONE_WRITE_GENERATED=1 cargo test --test views` (in server/): do not edit.

package com.breezone.app.data

/** A sample of each view of the contract, every field filled: the app's models must read them. */
object ApiSamples {
    const val AIR_QUALITY = """{"level":"good","mode":"off","threshold_high":1,"threshold_low":1,"value":1}"""
    const val COLOR_PROFILES = """{"blue":"x","red":"x","white":"x"}"""
    const val CONNECTION = """{"last_error":"x","last_seen":1,"latency_ms":1,"state":"online"}"""
    const val CONTRACT = """{"country":"x","currency":"x","offer":"x","power_kva":1,"supplier":"x"}"""
    const val DAY_PROFILE = """{"color":"x","id":"x","name":"x","slots":[{"name":"x","price":1,"start":"x"}]}"""
    const val DEVICE_VIEW = """{"available":true,"can_control":true,"id":1,"installation_id":1,"kind":"x","measures":{"device_id":1,"room_id":1},"name":"x","position":1,"reference":null,"role":"grid","room_id":1,"source":"x","source_id":1,"state":{},"traits":["x"]}"""
    const val INSTALLATION = """{"access":"local","connection":{"last_error":"x","last_seen":1,"latency_ms":1,"state":"online"},"controllable":true,"country":"x","enabled":true,"host":"x","id":1,"location":"x","name":"x","node":{"id":1,"name":"x","state":"online"},"port":1,"record_demand":true,"rights":{"admin":true,"routines":true,"schedules":true,"zones":true},"zones":[{"air_quality":{"level":"good","mode":"off","threshold_high":1,"threshold_low":1,"value":1},"airzone_name":"x","battery_low":true,"cool_setpoint":1,"demand":true,"double_setpoint":true,"errors":["x"],"group":"x","heat_setpoint":1,"humidity":1,"installation_id":1,"is_master":true,"max_temp":1,"min_temp":1,"mode":"stop","modes":["stop"],"name":"x","on":true,"position":1,"room_temp":1,"setpoint":1,"sleep":1,"speed":1,"speeds":[1],"step":1,"system_id":1,"units":"celsius","zone_id":1}]}"""
    const val INSTALLATION_USER_RIGHTS = """{"admin":true,"routines":true,"schedules":true,"zones":true}"""
    const val MEASURES = """{"device_id":1,"room_id":1}"""
    const val NODE_REF = """{"id":1,"name":"x","state":"online"}"""
    const val PRICE_AT = """{"color":"blue","day":"x","price":1,"profile":"x","profile_name":"x","slot":"x"}"""
    const val PRICE_PERIOD = """{"color":"blue","end":1,"price":1,"profile":"x","profile_name":"x","slot":"x","start":1}"""
    const val PRICE_SLOT = """{"name":"x","price":1,"start":"x"}"""
    const val PRICE_VIEW = """{"currency":"x","now":{"color":"blue","day":"x","price":1,"profile":"x","profile_name":"x","slot":"x"},"periods":[{"color":"blue","end":1,"price":1,"profile":"x","profile_name":"x","slot":"x","start":1}]}"""
    const val ROOM = """{"created_at":1,"icon":"x","id":1,"name":"x","position":1}"""
    const val TARIFF = """{"calendar":{"colors":{"blue":"x","red":"x","white":"x"},"day_start":"x","weekdays":["x"]},"export_price":1,"profiles":[{"color":"x","id":"x","name":"x","slots":[{"name":"x","price":1,"start":"x"}]}],"subscription":1}"""
    const val TARIFF_CALENDAR = """{"colors":{"blue":"x","red":"x","white":"x"},"day_start":"x","weekdays":["x"]}"""
    const val TARIFF_VERSION = """{"tariff":{"calendar":{"colors":{"blue":"x","red":"x","white":"x"},"day_start":"x","weekdays":["x"]},"export_price":1,"profiles":[{"color":"x","id":"x","name":"x","slots":[{"name":"x","price":1,"start":"x"}]}],"subscription":1},"valid_from":"x"}"""
    const val TARIFF_VIEW = """{"can_edit":true,"contract":{"country":"x","currency":"x","offer":"x","power_kva":1,"supplier":"x"},"versions":[{"tariff":{"calendar":{"colors":{"blue":"x","red":"x","white":"x"},"day_start":"x","weekdays":["x"]},"export_price":1,"profiles":[{"color":"x","id":"x","name":"x","slots":[{"name":"x","price":1,"start":"x"}]}],"subscription":1},"valid_from":"x"}]}"""
    const val ZONE = """{"air_quality":{"level":"good","mode":"off","threshold_high":1,"threshold_low":1,"value":1},"airzone_name":"x","battery_low":true,"cool_setpoint":1,"demand":true,"double_setpoint":true,"errors":["x"],"group":"x","heat_setpoint":1,"humidity":1,"installation_id":1,"is_master":true,"max_temp":1,"min_temp":1,"mode":"stop","modes":["stop"],"name":"x","on":true,"position":1,"room_temp":1,"setpoint":1,"sleep":1,"speed":1,"speeds":[1],"step":1,"system_id":1,"units":"celsius","zone_id":1}"""
}
