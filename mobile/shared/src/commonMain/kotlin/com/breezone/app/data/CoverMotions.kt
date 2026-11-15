package com.breezone.app.data

import kotlin.math.abs
import kotlin.math.roundToLong
import kotlin.time.TimeSource

/**
 * Where a shutter should be while it moves. Gateways like Legrand's only tell the position at the
 * end of a move, and only seconds after the command: the move in between is estimated, about 25 s
 * from closed to open. Same rules as the web (`web/src/lib/covers.ts`).
 */
class CoverMotions(private val now: () -> Long = monotonic()) {
    private data class Motion(val from: Double, val to: Long, val since: Long, var heard: Boolean)

    private val motions = mutableMapOf<String, Motion>()
    private val targets = mutableMapOf<String, Long?>()

    /** A command just sent: the shutter shows moving at once, from where it is shown. */
    fun start(key: String, state: CoverPositionState, to: Long) {
        val from = view(key, state).position
        if (from == to) motions.remove(key) else motions[key] = Motion(from.toDouble(), to, now(), heard = false)
    }

    fun stop(key: String) {
        motions.remove(key)
    }

    /** The state to show: the hardware's, or where the shutter should be by now while it moves. */
    fun view(key: String, s: CoverPositionState): CoverPositionState {
        val t = now()
        val known = key in targets
        val previous = targets[key]
        targets[key] = s.target
        val moving = s.moving == CoverPositionMoving.Opening || s.moving == CoverPositionMoving.Closing
        var m = motions[key]
        // A move started elsewhere is followed too, but not a target already there and never reached.
        if (s.target != null && s.target != s.position && m?.to != s.target && (moving || (known && previous != s.target))) {
            m = Motion(m?.let { estimate(it, t) } ?: s.position.toDouble(), s.target, t, heard = moving)
            motions[key] = m
        }
        if (m == null) return s
        if (moving) m.heard = true
        val stale = t - m.since > travel(m) + GRACE_MS
        if (s.position == m.to || (m.heard && !moving && s.target == m.to) || (stale && !moving)) {
            motions.remove(key)
            return s
        }
        val timed = estimate(m, t)
        val up = m.to > m.from
        // A device that tells its position on the way is believed when it is further along.
        val at = if (up) maxOf(timed, minOf(s.position, m.to).toDouble()) else minOf(timed, maxOf(s.position, m.to).toDouble())
        return s.copy(position = at.roundToLong(), target = m.to, moving = if (up) CoverPositionMoving.Opening else CoverPositionMoving.Closing)
    }

    private fun travel(m: Motion) = (abs(m.to - m.from) * MS_PER_PERCENT).toLong()

    private fun estimate(m: Motion, t: Long): Double {
        val travel = travel(m)
        val done = if (travel == 0L) 1.0 else minOf(1.0, (t - m.since).toDouble() / travel)
        return m.from + (m.to - m.from) * done
    }

    companion object {
        const val MS_PER_PERCENT = 250
        /** After the estimated end, how long the hardware has to confirm before what it says shows again. */
        const val GRACE_MS = 20_000L

        private fun monotonic(): () -> Long {
            val start = TimeSource.Monotonic.markNow()
            return { start.elapsedNow().inWholeMilliseconds }
        }
    }
}
