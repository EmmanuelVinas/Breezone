package com.breezone.app.ui.components

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.Reading
import com.breezone.app.ui.airQualityColor
import com.breezone.app.ui.airQualityLabel
import com.breezone.app.ui.clock
import com.breezone.app.ui.day
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.theme.Fz
import com.patrykandpatrick.vico.multiplatform.cartesian.CartesianChartHost
import com.patrykandpatrick.vico.multiplatform.cartesian.Zoom
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.Axis
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.HorizontalAxis
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.VerticalAxis
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianChartModelProducer
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianLayerRangeProvider
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianValueFormatter
import com.patrykandpatrick.vico.multiplatform.cartesian.data.lineSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.LineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLine
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberCartesianChart
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberVicoZoomState
import com.patrykandpatrick.vico.multiplatform.common.Fill
import com.patrykandpatrick.vico.multiplatform.common.component.rememberTextComponent
import com.patrykandpatrick.vico.multiplatform.common.data.ExtraStore
import com.breezone.app.resources.Res
import com.breezone.app.resources.air_quality_badge
import com.breezone.app.resources.air_quality_title
import com.breezone.app.resources.zone_humidity_label
import com.breezone.app.resources.zone_setpoint
import com.breezone.app.resources.zone_temperature
import org.jetbrains.compose.resources.stringResource
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.round

enum class HistorySeries { Temperature, Setpoint, Humidity, Air }

/** Longest time between two readings still drawn as one line (they are 5 minutes apart). */
private const val GAP_SECS = 1800L
private const val HOUR = 3600.0

private val TempKinds = ExtraStore.Key<List<HistorySeries>>()
private val AirLevels = ExtraStore.Key<List<Int>>()

private val LEVELS = listOf("good", "regular", "bad")

/** Consecutive readings with a value and no gap, as `(x in hours, value)` series. */
private fun runs(readings: List<Reading>, t0: Long, value: (Reading) -> Double?): List<List<Pair<Double, Double>>> {
    val result = mutableListOf<MutableList<Pair<Double, Double>>>()
    var previous: Reading? = null
    for (r in readings) {
        val v = value(r)
        if (v == null) {
            previous = null
            continue
        }
        val x = round((r.ts - t0) / HOUR * 10_000) / 10_000
        if (previous == null || r.ts - previous.ts > GAP_SECS) result.add(mutableListOf())
        result.last().add(x to v)
        previous = r
    }
    return result
}

/** Runs of the same air quality level; each holds the next reading, so that it reaches it. */
private fun airRuns(readings: List<Reading>, t0: Long): List<Pair<Int, List<Pair<Double, Double>>>> {
    val result = mutableListOf<Pair<Int, MutableList<Pair<Double, Double>>>>()
    var previous: Reading? = null
    for (r in readings) {
        val level = r.airQuality?.toInt()?.takeIf { it in 1..3 }
        val x = round((r.ts - t0) / HOUR * 10_000) / 10_000
        val continues = previous != null && r.ts - previous.ts <= GAP_SECS
        val last = result.lastOrNull()
        if (continues && last != null) last.second.add(x to 0.5)
        if (level != null && !(continues && last?.first == level)) result.add(level to mutableListOf(x to 0.5))
        previous = r.takeIf { level != null }
    }
    return result.filter { it.second.size >= 2 }
}

/**
 * Temperature and setpoint (left axis), humidity (right axis, 0-100 %) and air quality (a band at
 * the bottom). Pinch to zoom, drag to scroll, touch for the values. Hidden series stay in the
 * model, drawn transparent, so that toggling one does not rescale the chart.
 */
@Composable
fun HistoryChart(readings: List<Reading>, hours: Int, hidden: Set<HistorySeries> = emptySet(), modifier: Modifier = Modifier) {
    val t0 = readings.firstOrNull()?.ts ?: 0L
    val temperature = remember(readings) { runs(readings, t0) { it.roomTemp } }
    val setpoint = remember(readings) { runs(readings, t0) { r -> r.setpoint.takeIf { r.isOn == true } } }
    val humidity = remember(readings) { runs(readings, t0) { r -> r.humidity?.toDouble() } }
    val air = remember(readings) { airRuns(readings, t0) }

    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(readings) {
        if (temperature.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            lineSeries {
                (temperature + setpoint).forEach { s -> series(s.map { it.first }, s.map { it.second }) }
            }
            if (humidity.isNotEmpty()) lineSeries { humidity.forEach { s -> series(s.map { it.first }, s.map { it.second }) } }
            if (air.isNotEmpty()) lineSeries { air.forEach { (_, s) -> series(s.map { it.first }, s.map { it.second }) } }
            extras {
                it[TempKinds] = List(temperature.size) { HistorySeries.Temperature } + List(setpoint.size) { HistorySeries.Setpoint }
                it[AirLevels] = air.map { (level, _) -> level }
            }
        }
    }

    val heat = Fz.colors.heat
    val primary = MaterialTheme.colorScheme.primary
    val cool = Fz.colors.cool
    val muted = Fz.colors.muted
    val levelColors = LEVELS.map { airQualityColor(it) }
    fun shown(series: HistorySeries, color: Color) = if (series in hidden) Color.Transparent else color

    val temperatureLine = LineCartesianLayer.rememberLine(
        fill = LineCartesianLayer.LineFill.single(Fill(shown(HistorySeries.Temperature, heat))),
        stroke = LineCartesianLayer.LineStroke.Continuous(thickness = 2.5.dp, cap = StrokeCap.Round),
        pointConnector = LineCartesianLayer.PointConnector.cubic(),
    )
    val setpointLine = LineCartesianLayer.rememberLine(
        fill = LineCartesianLayer.LineFill.single(Fill(shown(HistorySeries.Setpoint, primary))),
        stroke = LineCartesianLayer.LineStroke.Dashed(thickness = 2.dp),
        pointConnector = StepConnector,
    )
    val humidityLine = LineCartesianLayer.rememberLine(
        fill = LineCartesianLayer.LineFill.single(Fill(shown(HistorySeries.Humidity, cool.copy(alpha = 0.7f)))),
        stroke = LineCartesianLayer.LineStroke.Continuous(thickness = 1.5.dp),
        pointConnector = LineCartesianLayer.PointConnector.cubic(),
    )
    val airLines = levelColors.map { color ->
        LineCartesianLayer.rememberLine(
            fill = LineCartesianLayer.LineFill.single(Fill(shown(HistorySeries.Air, color))),
            stroke = LineCartesianLayer.LineStroke.Continuous(thickness = 6.dp, cap = StrokeCap.Butt),
            pointConnector = LineCartesianLayer.PointConnector.Sharp,
        )
    }

    // The scale follows the temperatures instead of starting at 0°.
    val temps = (temperature + setpoint).flatten().map { it.second }
    val lo = floor(temps.minOrNull() ?: 0.0) - 1
    val hi = ceil(temps.maxOrNull() ?: 0.0) + 1
    val layers = buildList {
        add(
            rememberLineCartesianLayer(
                lineProvider = remember(temperatureLine, setpointLine) {
                    LineCartesianLayer.LineProvider { index, extras ->
                        if (extras.getOrNull(TempKinds)?.getOrNull(index) == HistorySeries.Setpoint) setpointLine else temperatureLine
                    }
                },
                pointSpacing = 4.dp,
                rangeProvider = remember(lo, hi) { CartesianLayerRangeProvider.fixed(minY = lo, maxY = hi) },
                verticalAxisPosition = Axis.Position.Vertical.Start,
            ),
        )
        if (humidity.isNotEmpty()) {
            add(
                rememberLineCartesianLayer(
                    lineProvider = remember(humidityLine) { LineCartesianLayer.LineProvider.series(humidityLine) },
                    pointSpacing = 4.dp,
                    rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0, maxY = 100.0) },
                    verticalAxisPosition = Axis.Position.Vertical.End,
                ),
            )
        }
        if (air.isNotEmpty()) {
            add(
                rememberLineCartesianLayer(
                    lineProvider = remember(airLines) {
                        LineCartesianLayer.LineProvider { index, extras ->
                            airLines[((extras.getOrNull(AirLevels)?.getOrNull(index) ?: 1) - 1).coerceIn(0, 2)]
                        }
                    },
                    pointSpacing = 4.dp,
                    // The band sits along the bottom of the plot.
                    rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0, maxY = 14.0) },
                ),
            )
        }
    }

    val label = TextStyle(fontSize = 10.sp, color = muted)
    val labels = HistoryLabels(
        temperature = stringResource(Res.string.zone_temperature),
        setpoint = stringResource(Res.string.zone_setpoint),
        humidity = stringResource(Res.string.zone_humidity_label),
        air = stringResource(Res.string.air_quality_title),
        levels = LEVELS.map { stringResource(Res.string.air_quality_badge, airQualityLabel(it)) },
    )
    val marker = rememberTextMarker(readings, hidden, labels) { x ->
        val ts = t0 + (x * HOUR).toLong()
        val r = readings.minByOrNull { abs(it.ts - ts) } ?: return@rememberTextMarker ""
        buildString {
            append(if (hours <= 24) clock(r.ts) else "${day(r.ts).dropLast(5)} ${clock(r.ts)}")
            if (HistorySeries.Temperature !in hidden) r.roomTemp?.let { append("\n${labels.temperature} ${formatTemp(it)}") }
            if (HistorySeries.Setpoint !in hidden && r.isOn == true) r.setpoint?.let { append("\n${labels.setpoint} ${formatTemp(it)}") }
            if (HistorySeries.Humidity !in hidden) r.humidity?.let { append("\n${labels.humidity} $it %") }
            if (HistorySeries.Air !in hidden) {
                r.airQuality?.toInt()?.takeIf { it in 1..3 }?.let { append("\n${labels.levels[it - 1]}") }
            }
        }
    }
    val timeFormatter = remember(t0, hours) {
        CartesianValueFormatter { _, x, _ ->
            val ts = t0 + (x * HOUR).toLong()
            if (hours <= 24) clock(ts) else day(ts).dropLast(5)
        }
    }
    val tempFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()}°" } }
    val percentFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.toInt()}%" } }
    val axisLabel = rememberTextComponent(style = label)

    CartesianChartHost(
        chart = rememberCartesianChart(
            *layers.toTypedArray(),
            startAxis = VerticalAxis.rememberStart(label = axisLabel, valueFormatter = tempFormatter, line = null, tick = null),
            endAxis = if (humidity.isNotEmpty()) {
                VerticalAxis.rememberEnd(
                    label = axisLabel,
                    valueFormatter = percentFormatter,
                    line = null,
                    tick = null,
                    guideline = null,
                    itemPlacer = remember { VerticalAxis.ItemPlacer.step({ 25.0 }) },
                )
            } else {
                null
            },
            bottomAxis = HorizontalAxis.rememberBottom(
                label = axisLabel,
                valueFormatter = timeFormatter,
                guideline = null,
                itemPlacer = remember(hours) { HorizontalAxis.ItemPlacer.aligned(spacing = { if (hours <= 24) 6 else 24 }) },
            ),
            marker = marker,
            getXStep = { _, _, _ -> 1.0 },
        ),
        modelProducer = producer,
        modifier = modifier.fillMaxWidth().height(220.dp),
        zoomState = rememberVicoZoomState(zoomEnabled = true, initialZoom = Zoom.Content),
        animateIn = false,
    )
}

private data class HistoryLabels(val temperature: String, val setpoint: String, val humidity: String, val air: String, val levels: List<String>)

/** A setpoint holds until the next one: draw it as steps. */
private val StepConnector = LineCartesianLayer.PointConnector { _, path, _, y1, x2, y2 ->
    path.lineTo(x2, y1)
    path.lineTo(x2, y2)
}
