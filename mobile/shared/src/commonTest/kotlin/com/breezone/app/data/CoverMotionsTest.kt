package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertEquals

class CoverMotionsTest {
    private var t = 0L
    private val motions = CoverMotions { t }
    private val atRest = CoverPositionState(position = 20, target = 20, moving = CoverPositionMoving.Stopped)

    @Test
    fun aLegrandShutterMovesAtOnceAndStaysAtItsTargetUntilTheGatewayTells() {
        motions.view("a", atRest)
        motions.start("a", atRest, 40)
        // The gateway's answer says nothing has moved yet: no going back to 20.
        val written = atRest.copy(target = 40)
        assertEquals(CoverPositionMoving.Opening, motions.view("a", written).moving)
        t = 2_500
        assertEquals(30L, motions.view("a", written).position)
        // Moving, the position still the one of the start.
        t = 4_000
        assertEquals(36L, motions.view("a", written.copy(moving = CoverPositionMoving.Opening)).position)
        t = 9_000
        assertEquals(40L, motions.view("a", written.copy(moving = CoverPositionMoving.Opening)).position)
        // Arrived.
        val arrived = CoverPositionState(position = 40, target = 40, moving = CoverPositionMoving.Stopped)
        assertEquals(arrived, motions.view("a", arrived))
    }

    @Test
    fun aTargetAlreadyThereWhenOpenedIsNotAMove() {
        val stoppedHalfway = CoverPositionState(position = 30, target = 100, moving = CoverPositionMoving.Stopped)
        assertEquals(stoppedHalfway, motions.view("a", stoppedHalfway))
    }

    @Test
    fun aMoveStartedElsewhereIsFollowedAndTheHardwareWinsWhenSilentTooLong() {
        motions.view("a", atRest)
        val closing = CoverPositionState(position = 20, target = 0, moving = CoverPositionMoving.Closing)
        t = 1_000
        motions.view("a", closing)
        t = 3_500
        assertEquals(10L, motions.view("a", closing).position)
        // Stopped short with the switch: what the hardware says shows again.
        val stopped = CoverPositionState(position = 20, target = 0, moving = CoverPositionMoving.Stopped)
        assertEquals(stopped, motions.view("a", stopped))
    }

    @Test
    fun aDeviceTellingItsPositionOnTheWayIsBelievedWhenAhead() {
        motions.view("a", atRest)
        motions.start("a", atRest, 100)
        t = 1_000
        assertEquals(60L, motions.view("a", CoverPositionState(position = 60, target = 100, moving = CoverPositionMoving.Opening)).position)
    }
}
