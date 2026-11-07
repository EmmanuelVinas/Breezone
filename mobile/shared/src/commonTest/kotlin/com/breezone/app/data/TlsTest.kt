package com.breezone.app.data

import kotlin.io.encoding.Base64
import kotlin.io.encoding.ExperimentalEncodingApi
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

@OptIn(ExperimentalEncodingApi::class)
class TlsTest {
    // Self-signed certificate made with OpenSSL (CN nas.example.org, two DNS names and one IP).
    private val der = Base64.decode("MIIBvjCCAWSgAwIBAgIUZfD/LN2+NHN9T9M3fDZ57IjaR3wwCgYIKoZIzj0EAwIwGjEYMBYGA1UEAwwPbmFzLmV4YW1wbGUub3JnMB4XDTI2MTEwNzE1NTUzNVoXDTI2MTIwNzE1NTUzNVowGjEYMBYGA1UEAwwPbmFzLmV4YW1wbGUub3JnMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEvEOPNfnQgd8rfSxzYyP5+HVtWCL4Dq3eps26QEzfPoEUZruksZoAUqLwS7KKql0g8nwMMeneyu3LWgZ6DL/TGKOBhzCBhDAdBgNVHQ4EFgQUyXEnS2y69VJH25LlMe5w+CyPrnUwHwYDVR0jBBgwFoAUyXEnS2y69VJH25LlMe5w+CyPrnUwDwYDVR0TAQH/BAUwAwEB/zAxBgNVHREEKjAogg9uYXMuZXhhbXBsZS5vcmeCD3d3dy5leGFtcGxlLm9yZ4cEwKgBFDAKBggqhkjOPQQDAgNIADBFAiARgLLCHlUxGcAumVRJ9NJM21bYzAzYvyd7yFW4dCYPewIhAPT+TxQmFN37D3GI0zPM18HI5zsUe00XTwco9zq+UJ1i")

    @Test
    fun readsWhatTheUserNeedsToDecide() {
        val info = parseCertificate(der)
        // Values given by `openssl x509 -noout -fingerprint -sha256 -dates`.
        assertEquals("D2:35:D0:BA:20:A6:E3:ED:62:AD:DF:3B:02:C8:A5:E0:4E:8C:BA:61:8A:55:6B:10:1D:FF:BB:BC:AB:36:30:A1", fingerprintText(info.fingerprint))
        assertEquals("nas.example.org", info.subject)
        assertEquals("nas.example.org", info.issuer)
        assertTrue(info.selfSigned)
        assertEquals(listOf("nas.example.org", "www.example.org", "192.168.1.20"), info.names)
        assertEquals(1794066935L, info.notBefore)
        assertEquals(1796658935L, info.notAfter)
    }

    @Test
    fun garbageIsNotFatal() {
        val info = parseCertificate(byteArrayOf(1, 2, 3))
        assertEquals(null, info.subject)
        assertEquals(64, info.fingerprint.length)
    }

    @Test
    fun trustIsPerHostAndPort() {
        TlsTrust.pin("https://nas.example.org", "ab")
        assertEquals("ab", TlsTrust.pinned("nas.example.org", 443))
        assertEquals(null, TlsTrust.pinned("nas.example.org", 8443))
        assertTrue(TlsTrust.isPinned("https://NAS.example.org/"))
        TlsTrust.pin("https://nas.example.org", null)
        assertEquals(null, TlsTrust.pinned("nas.example.org", 443))
    }

    @Test
    fun recognisesPlatformErrors() {
        assertEquals(NetworkProblem.Certificate, classify(Exception("Hostname nas.example.org not verified")))
        assertEquals(NetworkProblem.Certificate, classify(Exception("Error Domain=NSURLErrorDomain Code=-1202 \"The certificate is invalid\"")))
        assertEquals(NetworkProblem.UnknownHost, classify(Exception("Unable to resolve host \"nope.example\"")))
        assertEquals(NetworkProblem.Refused, classify(Exception("Failed to connect to /192.168.1.20:8427")))
        assertEquals(NetworkProblem.Timeout, classify(Exception("Connect timeout has expired")))
        assertEquals(null, classify(Exception("something else")))
    }
}
