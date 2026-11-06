package com.breezone.app.ui.components

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.Reading
import com.breezone.app.ui.clock
import com.breezone.app.ui.theme.Fz
import kotlin.math.ceil
import kotlin.math.floor

/**
 * Temperature (solid), setpoint when on (dashed) and humidity (faint, 0-100 %).
 * Paths, grid and labels are built once per data set / size in `drawWithCache`.
 */
@Composable
fun HistoryChart(readings: List<Reading>, modifier: Modifier = Modifier) {
    val measurer = rememberTextMeasurer()
    val heat = Fz.colors.heat
    val cool = Fz.colors.cool
    val primary = MaterialTheme.colorScheme.primary
    val grid = MaterialTheme.colorScheme.outlineVariant
    val label = TextStyle(fontSize = 10.sp, color = Fz.colors.muted)
    Spacer(
        modifier
            .fillMaxWidth()
            .height(200.dp)
            .drawWithCache {
                val temps = readings.flatMap { r -> listOfNotNull(r.roomTemp, r.setpoint.takeIf { r.isOn == true }) }
                if (readings.size < 2 || temps.isEmpty()) return@drawWithCache onDrawBehind {}
                val left = 30.dp.toPx()
                val bottom = 18.dp.toPx()
                val w = size.width - left
                val h = size.height - bottom
                val t0 = readings.first().ts
                val t1 = readings.last().ts.coerceAtLeast(t0 + 1)
                val lo = floor(temps.min()) - 1
                val hi = ceil(temps.max()) + 1
                fun x(ts: Long) = left + (ts - t0).toFloat() / (t1 - t0) * w
                fun y(v: Double) = (h - (v - lo) / (hi - lo) * h).toFloat()
                fun yh(v: Double) = (h - v / 100.0 * h).toFloat()

                val gridLines = (0..3).map { i -> lo + (hi - lo) * i / 3 }
                val yLabels = gridLines.map { v -> measurer.measure("${v.toInt()}°", label) to Offset(0f, y(v) - 7.dp.toPx()) }
                val xLabels = listOf(t0, (t0 + t1) / 2, t1).mapIndexed { i, ts ->
                    val layout = measurer.measure(clock(ts), label)
                    val tw = layout.size.width
                    val px = when (i) {
                        0 -> left
                        2 -> size.width - tw
                        else -> x(ts) - tw / 2
                    }
                    layout to Offset(px, h + 4.dp.toPx())
                }

                val temperature = Path()
                val setpoint = Path()
                val humidity = Path()
                var tempStarted = false
                var spStarted = false
                var humStarted = false
                var lastSpY = 0f
                readings.forEach { r ->
                    val px = x(r.ts)
                    r.roomTemp?.let { v ->
                        if (tempStarted) temperature.lineTo(px, y(v)) else temperature.moveTo(px, y(v)).also { tempStarted = true }
                    }
                    val sp = r.setpoint.takeIf { r.isOn == true }
                    if (sp == null) {
                        spStarted = false
                    } else {
                        val py = y(sp)
                        if (spStarted) {
                            setpoint.lineTo(px, lastSpY) // stepped line
                            setpoint.lineTo(px, py)
                        } else {
                            setpoint.moveTo(px, py)
                            spStarted = true
                        }
                        lastSpY = py
                    }
                    r.humidity?.let { v ->
                        if (humStarted) humidity.lineTo(px, yh(v.toDouble())) else humidity.moveTo(px, yh(v.toDouble())).also { humStarted = true }
                    }
                }
                val tempStroke = Stroke(2.5.dp.toPx(), cap = StrokeCap.Round)
                val spStroke = Stroke(2.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(14f, 10f)))
                val humStroke = Stroke(1.5.dp.toPx())
                val humColor = cool.copy(alpha = 0.5f)

                onDrawBehind {
                    gridLines.forEach { v -> drawLine(grid, Offset(left, y(v)), Offset(size.width, y(v)), 1f) }
                    yLabels.forEach { (layout, at) -> drawText(layout, topLeft = at) }
                    xLabels.forEach { (layout, at) -> drawText(layout, topLeft = at) }
                    drawPath(humidity, humColor, style = humStroke)
                    drawPath(setpoint, primary, style = spStroke)
                    drawPath(temperature, heat, style = tempStroke)
                }
            },
    )
}
