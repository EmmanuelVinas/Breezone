// Generated from server/contracts/views.json by `BREEZONE_WRITE_GENERATED=1 cargo test --test views` (in server/): do not edit.

package com.breezone.app.data

/** A sample of each view of the contract, every field filled: the app's models must read them. */
object ApiSamples {
    const val AIR_QUALITY = """{"level":"good","mode":"off","threshold_high":1,"threshold_low":1,"value":1}"""
    const val CONNECTION = """{"last_error":"x","last_seen":1,"latency_ms":1,"state":"online"}"""
    const val DEVICE_VIEW = """{"available":true,"can_control":true,"id":1,"installation_id":1,"kind":"x","measures":{"device_id":1,"room_id":1},"name":"x","position":1,"reference":null,"role":"grid","room_id":1,"source":"x","source_id":1,"state":{},"traits":["x"]}"""
    const val INSTALLATION = """{"access":"local","connection":{"last_error":"x","last_seen":1,"latency_ms":1,"state":"online"},"controllable":true,"country":"x","enabled":true,"host":"x","id":1,"location":"x","name":"x","node":{"id":1,"name":"x","state":"online"},"port":1,"record_demand":true,"rights":{"admin":true,"routines":true,"schedules":true,"zones":true},"zones":[{"air_quality":{"level":"good","mode":"off","threshold_high":1,"threshold_low":1,"value":1},"airzone_name":"x","battery_low":true,"cool_setpoint":1,"demand":true,"double_setpoint":true,"errors":["x"],"group":"x","heat_setpoint":1,"humidity":1,"installation_id":1,"is_master":true,"max_temp":1,"min_temp":1,"mode":"stop","modes":["stop"],"name":"x","on":true,"position":1,"room_temp":1,"setpoint":1,"sleep":1,"speed":1,"speeds":[1],"step":1,"system_id":1,"units":"celsius","zone_id":1}]}"""
    const val INSTALLATION_USER_RIGHTS = """{"admin":true,"routines":true,"schedules":true,"zones":true}"""
    const val MEASURES = """{"device_id":1,"room_id":1}"""
    const val NODE_REF = """{"id":1,"name":"x","state":"online"}"""
    const val ROOM = """{"created_at":1,"icon":"x","id":1,"name":"x","position":1}"""
    const val ZONE = """{"air_quality":{"level":"good","mode":"off","threshold_high":1,"threshold_low":1,"value":1},"airzone_name":"x","battery_low":true,"cool_setpoint":1,"demand":true,"double_setpoint":true,"errors":["x"],"group":"x","heat_setpoint":1,"humidity":1,"installation_id":1,"is_master":true,"max_temp":1,"min_temp":1,"mode":"stop","modes":["stop"],"name":"x","on":true,"position":1,"room_temp":1,"setpoint":1,"sleep":1,"speed":1,"speeds":[1],"step":1,"system_id":1,"units":"celsius","zone_id":1}"""
}
