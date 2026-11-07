package com.breezone.app.data

import kotlin.concurrent.Volatile

/** SHA-256 of `bytes` (platform implementation). */
expect fun sha256(bytes: ByteArray): ByteArray

/** Lowercase hex SHA-256 of a certificate (DER), as the server and the web UI show it. */
fun fingerprint(der: ByteArray): String = sha256(der).joinToString("") { (it.toInt() and 0xff).toString(16).padStart(2, '0') }

/** `ab12…` → `AB:12:…`, as browsers show fingerprints. */
fun fingerprintText(fp: String): String = fp.uppercase().chunked(2).joinToString(":")

/** What the user sees before trusting a certificate that fails the usual HTTPS checks. */
data class CertificateInfo(
    val fingerprint: String,
    val subject: String?,
    val names: List<String>,
    val issuer: String?,
    /** Epoch seconds. */
    val notBefore: Long?,
    val notAfter: Long?,
    val selfSigned: Boolean,
)

/**
 * HTTPS certificates of Breezone servers.
 *
 * The platform HTTP engines record the certificate each server presents, and accept a
 * certificate that fails the usual checks (self-signed, name not covered…) only when the user
 * trusted that exact certificate (pinned by its SHA-256) for that server.
 */
object TlsTrust {
    @Volatile private var pins: Map<String, String> = emptyMap()
    @Volatile private var seen: Map<String, ByteArray> = emptyMap()

    private fun key(host: String, port: Int) = "${host.lowercase()}:$port"

    /** `host:port` of an URL (default HTTPS port 443). */
    fun keyOf(url: String): String? {
        val u = normalizeUrl(url)
        if (!u.startsWith("https://")) return null
        val hostPort = u.removePrefix("https://").substringBefore('/')
        val host = hostPort.substringBeforeLast(':', hostPort).removePrefix("[").removeSuffix("]")
        val port = hostPort.substringAfterLast(':', "").toIntOrNull() ?: 443
        return key(host, port)
    }

    fun pinned(host: String, port: Int): String? = pins[key(host, port)]

    fun record(host: String, port: Int, der: ByteArray) {
        seen = seen + (key(host, port) to der)
    }

    /** Trusts `fingerprint` for the server at `url` (null: back to the usual checks). */
    fun pin(url: String, fingerprint: String?) {
        val k = keyOf(url) ?: return
        pins = if (fingerprint == null) pins - k else pins + (k to fingerprint)
    }

    /** A certificate was trusted for the server at `url`. */
    fun isPinned(url: String): Boolean = keyOf(url)?.let { it in pins } == true

    /** Certificate the server at `url` presented last, if any. */
    fun presented(url: String): CertificateInfo? = keyOf(url)?.let { seen[it] }?.let(::parseCertificate)
}

// ---------------------------------------------------------------------------------------
// Minimal X.509 reading (DER): subject and issuer common names, names covered, dates.
// ---------------------------------------------------------------------------------------

private class Der(val tag: Int, val bytes: ByteArray, val start: Int, val end: Int) {
    fun children(): List<Der> {
        val out = mutableListOf<Der>()
        var i = start
        while (i < end) {
            val node = readDer(bytes, i) ?: break
            out += node
            i = node.end
        }
        return out
    }
    fun content(): ByteArray = bytes.copyOfRange(start, end)
}

private fun readDer(b: ByteArray, at: Int): Der? {
    if (at + 2 > b.size) return null
    val tag = b[at].toInt() and 0xff
    var len = b[at + 1].toInt() and 0xff
    var i = at + 2
    if (len and 0x80 != 0) {
        val n = len and 0x7f
        if (n == 0 || n > 4 || i + n > b.size) return null
        len = 0
        repeat(n) { len = (len shl 8) or (b[i++].toInt() and 0xff) }
    }
    if (len < 0 || i + len > b.size) return null
    return Der(tag, b, i, i + len)
}

private val CN_OID = byteArrayOf(0x55, 0x04, 0x03)
private val SAN_OID = byteArrayOf(0x55, 0x1d, 0x11)

private fun commonName(name: Der): String? =
    name.children().flatMap { it.children() }.firstOrNull { atv ->
        atv.children().firstOrNull()?.let { it.tag == 0x06 && it.content().contentEquals(CN_OID) } == true
    }?.children()?.getOrNull(1)?.content()?.decodeToString()

private fun time(t: Der): Long? {
    val s = t.content().decodeToString()
    val full = when (t.tag) {
        0x17 -> (if ((s.take(2).toIntOrNull() ?: return null) < 50) "20" else "19") + s // UTCTime
        0x18 -> s // GeneralizedTime
        else -> return null
    }
    if (full.length < 14) return null
    val y = full.substring(0, 4).toIntOrNull() ?: return null
    val mo = full.substring(4, 6).toIntOrNull() ?: return null
    val d = full.substring(6, 8).toIntOrNull() ?: return null
    val h = full.substring(8, 10).toIntOrNull() ?: return null
    val mi = full.substring(10, 12).toIntOrNull() ?: return null
    val se = full.substring(12, 14).toIntOrNull() ?: return null
    return daysFromCivil(y, mo, d) * 86_400L + h * 3600L + mi * 60L + se
}

/** Days since 1970-01-01 (proleptic Gregorian calendar). */
private fun daysFromCivil(year: Int, month: Int, day: Int): Long {
    val y = if (month <= 2) year - 1 else year
    val era = (if (y >= 0) y else y - 399) / 400
    val yoe = y - era * 400
    val doy = (153 * (month + (if (month > 2) -3 else 9)) + 2) / 5 + day - 1
    val doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
    return era * 146_097L + doe - 719_468L
}

fun parseCertificate(der: ByteArray): CertificateInfo {
    var info = CertificateInfo(fingerprint(der), null, emptyList(), null, null, null, false)
    runCatching {
        val tbs = readDer(der, 0)!!.children()[0].children()
        // [0] version is optional: the serial number is the first INTEGER.
        val fields = if (tbs[0].tag == 0xa0) tbs.drop(1) else tbs
        val issuer = fields[2]
        val validity = fields[3].children()
        val subject = fields[4]
        val names = mutableListOf<String>()
        fields.firstOrNull { it.tag == 0xa3 }?.children()?.firstOrNull()?.children()?.forEach { ext ->
            val parts = ext.children()
            if (parts.firstOrNull()?.content()?.contentEquals(SAN_OID) == true) {
                val octets = parts.last()
                readDer(octets.content(), 0)?.children()?.forEach { gn ->
                    when (gn.tag) {
                        0x82 -> names += gn.content().decodeToString() // dNSName
                        0x87 -> gn.content().let { ip ->
                            if (ip.size == 4) names += ip.joinToString(".") { (it.toInt() and 0xff).toString() }
                        }
                    }
                }
            }
        }
        info = info.copy(
            subject = commonName(subject),
            issuer = commonName(issuer),
            names = names,
            notBefore = time(validity[0]),
            notAfter = time(validity[1]),
            selfSigned = issuer.content().contentEquals(subject.content()),
        )
    }
    return info
}
