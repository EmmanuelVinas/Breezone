package com.breezone.app.ui

import com.breezone.app.data.Reading
import com.breezone.app.ui.components.ZoneState
import com.breezone.app.ui.components.zoneStates
import kotlin.test.Test
import kotlin.test.assertEquals

class ZoneStatesTest {
    private fun readings(temps: List<Double>, mode: Long, demand: Long? = null) =
        temps.mapIndexed { i, t -> Reading(ts = i * 300L, roomTemp = t, setpoint = 19.0, isOn = true, mode = mode, demandSecs = demand) }

    private fun marks(states: List<ZoneState?>) = states.joinToString("") {
        when (it) {
            ZoneState.Heating -> "H"
            ZoneState.Cooling -> "C"
            else -> "."
        }
    }

    // Every 5 min: falls from 19.4 to 18.6, rises to 19.6, falls again (a setpoint of 19°).
    private val curve = listOf(19.4, 19.3, 19.2, 19.1, 19.0, 18.9, 18.8, 18.7, 18.6, 18.6, 18.7, 18.9, 19.1, 19.3, 19.5, 19.6, 19.6, 19.5, 19.4, 19.3)

    @Test
    fun heatingIsWhileTheTemperatureRisesFromBelowTheSetpoint() {
        assertEquals(".........HHHHHH.....", marks(zoneStates(readings(curve, 3))))
    }

    @Test
    fun theFallAfterAnOvershootIsNotCoolingInAuto() {
        assertEquals(".........HHHHHH.....", marks(zoneStates(readings(curve, 7))))
    }

    @Test
    fun theSunWarmingARoomAboveItsSetpointIsNotHeating() {
        assertEquals(".....", marks(zoneStates(readings(listOf(19.5, 19.7, 19.9, 20.1, 20.3), 3))))
    }

    @Test
    fun aRecordedDemandWins() {
        // Flat at the setpoint, yet the installation recorded 2 min of demand: heating.
        assertEquals(ZoneState.Heating, zoneStates(readings(listOf(19.0, 19.0), 3, demand = 120))[0])
        assertEquals(ZoneState.Comfort, zoneStates(readings(listOf(18.0, 19.0), 3, demand = 0))[0], "a rise without demand")
    }
}
