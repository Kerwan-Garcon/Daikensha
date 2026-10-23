package android.annotation

/** Stand-in for the Android SDK's lint annotation, which has no effect at run time. */
@Retention(AnnotationRetention.SOURCE)
annotation class SuppressLint(vararg val value: String)
