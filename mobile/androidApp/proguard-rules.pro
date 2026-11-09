# Libraries (Compose, Ktor, OkHttp, kotlinx.serialization, coroutines, Koin) ship their own
# consumer rules; only app-specific rules belong here.

# Keep file and line numbers so crash reports stay readable once deobfuscated.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile

# Error messages show the exception class names (data/Errors.kt): keep them unobfuscated.
-keepnames class * extends java.lang.Throwable
