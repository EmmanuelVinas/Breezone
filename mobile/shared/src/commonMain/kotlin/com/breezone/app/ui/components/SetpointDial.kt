package com.breezone.app.ui.components

import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.Zone
import com.breezone.app.resources.*
import com.breezone.app.ui.clampSetpoint
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.theme.Fz
import com.breezone.app.ui.theme.TempStyle
import org.jetbrains.compose.resources.stringResource
import kotlin.math.PI
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin

private const val START = 135f
private const val SWEEP = 270f
private const val TICKS = 30

/**
 * Circular thermostat: drag along the arc (or tap on it) to change the setpoint.
 *
 * Performance: everything that only depends on the size (ticks, brush, geometry) is built once in
 * `drawWithCache`; the current value is read through [value] inside the draw phase, so dragging
 * only redraws the arc and recomposes the small centre label.
 */
@Composable
fun SetpointDial(
    zone: Zone,
    value: () -> Double,
    color: Color,
    onChange: (Double) -> Unit,
    onCommit: (Double) -> Unit,
    modifier: Modifier = Modifier,
) {
    val min = zone.minTemp ?: 15.0
    val max = zone.maxTemp ?: 30.0
    val span = (max - min).coerceAtLeast(0.1)
    var dragging by remember { mutableStateOf(false) }
    val currentZone by rememberUpdatedState(zone)
    val currentOnChange by rememberUpdatedState(onChange)
    val currentOnCommit by rememberUpdatedState(onCommit)
    val track = MaterialTheme.colorScheme.surfaceVariant
    val tick = Fz.colors.muted.copy(alpha = 0.4f)
    val knobFill = Fz.colors.card
    val roomColor = MaterialTheme.colorScheme.onSurface
    val room = zone.roomTemp

    fun frac(v: Double) = ((v - min) / span).coerceIn(0.0, 1.0).toFloat()

    fun valueAt(offset: Offset, size: IntSize): Double {
        val dx = offset.x - size.width / 2f
        val dy = offset.y - size.height / 2f
        var deg = (atan2(dy, dx) * 180 / PI).toFloat()
        if (deg < 0) deg += 360f
        var rel = (deg - START + 360f) % 360f
        if (rel > SWEEP) rel = if (rel > SWEEP + (360f - SWEEP) / 2) 0f else SWEEP
        return clampSetpoint(currentZone, min + rel / SWEEP * (max - min))
    }

    Box(modifier.aspectRatio(1f), contentAlignment = Alignment.Center) {
        Spacer(
            Modifier
                .fillMaxSize()
                .pointerInput(min, max) {
                    detectDragGestures(
                        onDragStart = { dragging = true; currentOnChange(valueAt(it, size)) },
                        onDrag = { change, _ -> currentOnChange(valueAt(change.position, size)) },
                        onDragEnd = { dragging = false; currentOnCommit(Double.NaN) },
                        onDragCancel = { dragging = false; currentOnCommit(Double.NaN) },
                    )
                }
                .pointerInput(min, max) {
                    detectTapGestures { currentOnCommit(valueAt(it, size)) }
                }
                .drawWithCache {
                    val center = Offset(size.width / 2, size.height / 2)
                    val stroke = 16.dp.toPx()
                    val r = size.minDimension / 2 - 26.dp.toPx()
                    val topLeft = Offset(center.x - r, center.y - r)
                    val arcSize = Size(r * 2, r * 2)
                    fun polar(deg: Float, radius: Float) = Offset(
                        center.x + radius * cos(deg * PI / 180).toFloat(),
                        center.y + radius * sin(deg * PI / 180).toFloat(),
                    )
                    val tickLines = (0..TICKS).map { i ->
                        val a = START + SWEEP * i / TICKS
                        val long = i % 5 == 0
                        Triple(polar(a, r + 14.dp.toPx()), polar(a, r + (if (long) 6 else 10).dp.toPx()), if (long) 2.dp.toPx() else 1.dp.toPx())
                    }
                    val trackStroke = Stroke(stroke, cap = StrokeCap.Round)
                    val glowStroke = Stroke(stroke * 2.2f, cap = StrokeCap.Round)
                    val knobStroke = Stroke(4.dp.toPx())
                    val brush = Brush.sweepGradient(listOf(color.copy(alpha = 0.5f), color, color.copy(alpha = 0.5f)), center)
                    val glow = color.copy(alpha = 0.25f)
                    val roomPoint = room?.let { polar(START + SWEEP * frac(it), r) }
                    onDrawBehind {
                        tickLines.forEach { (a, b, w) -> drawLine(tick, a, b, strokeWidth = w) }
                        drawArc(track, START, SWEEP, false, topLeft, arcSize, style = trackStroke)
                        val sweep = SWEEP * frac(value())
                        drawArc(glow, START, sweep, false, topLeft, arcSize, style = glowStroke)
                        drawArc(brush, START, sweep, false, topLeft, arcSize, style = trackStroke)
                        roomPoint?.let { drawCircle(roomColor, 4.dp.toPx(), it) }
                        val knob = polar(START + sweep, r)
                        val knobRadius = (if (dragging) 15 else 13).dp.toPx()
                        drawCircle(knobFill, knobRadius, knob)
                        drawCircle(color, knobRadius, knob, style = knobStroke)
                    }
                },
        )
        DialLabel(value, room)
    }
}

/** Centre text: the only part recomposed while the setpoint changes. */
@Composable
private fun DialLabel(value: () -> Double, room: Double?) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(stringResource(Res.string.zone_setpoint_caps), fontSize = 12.sp, letterSpacing = 3.sp, color = Fz.colors.muted)
        Text(formatTemp(value()), style = TempStyle, fontSize = 60.sp, letterSpacing = (-2).sp)
        Text(stringResource(Res.string.zone_ambient, formatTemp(room)), fontSize = 14.sp, color = Fz.colors.muted)
    }
}
