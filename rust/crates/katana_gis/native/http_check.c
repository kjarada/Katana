/*
 * The check of the HTTP declarations in src/ffi/ (http.rs: GDAL's HTTP
 * client; curl.rs: libcurl), compiled by build.rs against the headers of the
 * GDAL and the libcurl the crate links, as native/ffi_check.c checks the rest
 * of src/ffi/. It is no code: every line is a _Static_assert, so a header that
 * changes, or a declaration transcribed wrongly, fails the build here rather
 * than misbehaving at run time.
 *
 *   - Each function the Rust declares, by its type, compared by _Generic
 *     (C11 6.5.1.1) with the header's declaration - curl_easy_setopt and
 *     curl_easy_getinfo as the variadics they are. CPLHTTPSetOptions is C++
 *     only and is checked by its compiler in native/http_shim.cpp.
 *   - Each callback type the Rust writes a function for.
 *   - Each type the Rust takes for another: the enumerations an int, the
 *     64-bit curl_off_t.
 *   - The #[repr(C)] layouts of CPLHTTPResult and CPLMimePart.
 *   - Each constant's value: every CURLOPT_* number, with its type class,
 *     every CURLE_* code, CURLINFO_RESPONSE_CODE and the rest.
 *
 * The numbers here are the Rust's: change one, change both.
 */

/* The plain declarations, without curl's GCC type-checking macros. */
#define CURL_DISABLE_TYPECHECK 1

#include <stddef.h>

#include <cpl_http.h>
#include <cpl_progress.h>
#include <curl/curl.h>

#define KATANA_SAME(value, expected) _Static_assert((value) == (expected), #value " is " #expected)
#define KATANA_DECLARED(function, type)                                                            \
    _Static_assert(_Generic(&(function), type: 1, default: 0), #function " is " #type)
#define KATANA_TYPE(name, type)                                                                    \
    _Static_assert(_Generic((name)0, type: 1, default: 0), #name " is " #type)

/* ---- the types the Rust takes for others ------------------------------------------------- */

KATANA_SAME(sizeof(CURLcode), sizeof(int));
KATANA_SAME(sizeof(CURLoption), sizeof(int));
KATANA_SAME(sizeof(CURLINFO), sizeof(int));
KATANA_SAME(sizeof(curl_off_t), 8);
KATANA_SAME(sizeof(GByte), 1);

/* ---- layouts -------------------------------------------------------------------------------- */

#if defined(__LP64__) || defined(_WIN64)
KATANA_SAME(sizeof(CPLMimePart), 24);
KATANA_SAME(offsetof(CPLMimePart, papszHeaders), 0);
KATANA_SAME(offsetof(CPLMimePart, pabyData), 8);
KATANA_SAME(offsetof(CPLMimePart, nDataLen), 16);

KATANA_SAME(sizeof(CPLHTTPResult), 64);
KATANA_SAME(offsetof(CPLHTTPResult, nStatus), 0);
KATANA_SAME(offsetof(CPLHTTPResult, pszContentType), 8);
KATANA_SAME(offsetof(CPLHTTPResult, pszErrBuf), 16);
KATANA_SAME(offsetof(CPLHTTPResult, nDataLen), 24);
KATANA_SAME(offsetof(CPLHTTPResult, nDataAlloc), 28);
KATANA_SAME(offsetof(CPLHTTPResult, pabyData), 32);
KATANA_SAME(offsetof(CPLHTTPResult, papszHeaders), 40);
KATANA_SAME(offsetof(CPLHTTPResult, nMimePartCount), 48);
KATANA_SAME(offsetof(CPLHTTPResult, pasMimePart), 56);
#else
#error "CPLHTTPResult's layout is written for the 64-bit platforms only"
#endif

/* ---- constants ------------------------------------------------------------------------------ */

KATANA_SAME(CURLE_OK, 0);
KATANA_SAME(CURLE_FAILED_INIT, 2);
KATANA_SAME(CURLE_COULDNT_RESOLVE_PROXY, 5);
KATANA_SAME(CURLE_COULDNT_RESOLVE_HOST, 6);
KATANA_SAME(CURLE_COULDNT_CONNECT, 7);
KATANA_SAME(CURLE_PARTIAL_FILE, 18);
KATANA_SAME(CURLE_WRITE_ERROR, 23);
KATANA_SAME(CURLE_OUT_OF_MEMORY, 27);
KATANA_SAME(CURLE_OPERATION_TIMEDOUT, 28);
KATANA_SAME(CURLE_GOT_NOTHING, 52);
KATANA_SAME(CURLE_SEND_ERROR, 55);
KATANA_SAME(CURLE_RECV_ERROR, 56);
KATANA_SAME(CURLE_PEER_FAILED_VERIFICATION, 60);
KATANA_SAME(CURLE_SSL_CACERT_BADFILE, 77);

KATANA_SAME(CURL_ERROR_SIZE, 256);
KATANA_SAME(CURL_NETRC_IGNORED, 0);
KATANA_SAME(CURLAUTH_NONE, 0);
KATANA_SAME(CURLINFO_RESPONSE_CODE, 0x200000 + 2);

KATANA_SAME(CURLOPTTYPE_LONG, 0);
KATANA_SAME(CURLOPTTYPE_VALUES, 0);
KATANA_SAME(CURLOPTTYPE_OBJECTPOINT, 10000);
KATANA_SAME(CURLOPTTYPE_STRINGPOINT, 10000);
KATANA_SAME(CURLOPTTYPE_SLISTPOINT, 10000);
KATANA_SAME(CURLOPTTYPE_CBPOINT, 10000);
KATANA_SAME(CURLOPTTYPE_FUNCTIONPOINT, 20000);

/* A long each (LongOption). */
KATANA_SAME(CURLOPT_VERBOSE, CURLOPTTYPE_LONG + 41);
KATANA_SAME(CURLOPT_NOPROGRESS, CURLOPTTYPE_LONG + 43);
KATANA_SAME(CURLOPT_NETRC, CURLOPTTYPE_VALUES + 51);
KATANA_SAME(CURLOPT_FOLLOWLOCATION, CURLOPTTYPE_LONG + 52);
KATANA_SAME(CURLOPT_SSL_VERIFYPEER, CURLOPTTYPE_LONG + 64);
KATANA_SAME(CURLOPT_HTTPGET, CURLOPTTYPE_LONG + 80);
KATANA_SAME(CURLOPT_SSL_VERIFYHOST, CURLOPTTYPE_LONG + 81);
KATANA_SAME(CURLOPT_NOSIGNAL, CURLOPTTYPE_LONG + 99);
KATANA_SAME(CURLOPT_HTTPAUTH, CURLOPTTYPE_VALUES + 107);

/* A pointer each (PointerOption). */
KATANA_SAME(CURLOPT_WRITEDATA, CURLOPTTYPE_CBPOINT + 1);
KATANA_SAME(CURLOPT_URL, CURLOPTTYPE_STRINGPOINT + 2);
KATANA_SAME(CURLOPT_USERPWD, CURLOPTTYPE_STRINGPOINT + 5);
KATANA_SAME(CURLOPT_ERRORBUFFER, CURLOPTTYPE_OBJECTPOINT + 10);
KATANA_SAME(CURLOPT_COOKIE, CURLOPTTYPE_STRINGPOINT + 22);
KATANA_SAME(CURLOPT_HTTPHEADER, CURLOPTTYPE_SLISTPOINT + 23);
KATANA_SAME(CURLOPT_SSLCERT, CURLOPTTYPE_STRINGPOINT + 25);
KATANA_SAME(CURLOPT_KEYPASSWD, CURLOPTTYPE_STRINGPOINT + 26);
KATANA_SAME(CURLOPT_HEADERDATA, CURLOPTTYPE_CBPOINT + 29);
KATANA_SAME(CURLOPT_COOKIEFILE, CURLOPTTYPE_STRINGPOINT + 31);
KATANA_SAME(CURLOPT_XFERINFODATA, CURLOPTTYPE_CBPOINT + 57);
KATANA_SAME(CURLOPT_COOKIEJAR, CURLOPTTYPE_STRINGPOINT + 82);
KATANA_SAME(CURLOPT_SSLKEY, CURLOPTTYPE_STRINGPOINT + 87);
KATANA_SAME(CURLOPT_ACCEPT_ENCODING, CURLOPTTYPE_STRINGPOINT + 102);
KATANA_SAME(CURLOPT_NETRC_FILE, CURLOPTTYPE_STRINGPOINT + 118);
KATANA_SAME(CURLOPT_XOAUTH2_BEARER, CURLOPTTYPE_STRINGPOINT + 220);

/* A function pointer each (WriteFunctionOption, and the progress callback's). */
KATANA_SAME(CURLOPT_WRITEFUNCTION, CURLOPTTYPE_FUNCTIONPOINT + 11);
KATANA_SAME(CURLOPT_HEADERFUNCTION, CURLOPTTYPE_FUNCTIONPOINT + 79);
KATANA_SAME(CURLOPT_XFERINFOFUNCTION, CURLOPTTYPE_FUNCTIONPOINT + 219);

/* ---- callbacks ------------------------------------------------------------------------------ */

KATANA_TYPE(curl_write_callback, size_t (*)(char *, size_t, size_t, void *));
KATANA_TYPE(curl_xferinfo_callback, int (*)(void *, curl_off_t, curl_off_t, curl_off_t, curl_off_t));
KATANA_TYPE(CPLHTTPFetchWriteFunc, size_t (*)(void *, size_t, size_t, void *));
KATANA_TYPE(GDALProgressFunc, int (*)(double, const char *, void *));

/* ---- http.rs ---------------------------------------------------------------------------------- */

KATANA_DECLARED(CPLHTTPFetchEx, CPLHTTPResult *(*)(const char *, CSLConstList, GDALProgressFunc,
                                                    void *, CPLHTTPFetchWriteFunc, void *));
KATANA_DECLARED(CPLHTTPDestroyResult, void (*)(CPLHTTPResult *));

/* ---- curl.rs ---------------------------------------------------------------------------------- */

KATANA_DECLARED(curl_easy_init, CURL *(*)(void));
KATANA_DECLARED(curl_easy_setopt, CURLcode (*)(CURL *, CURLoption, ...));
KATANA_DECLARED(curl_easy_perform, CURLcode (*)(CURL *));
KATANA_DECLARED(curl_easy_getinfo, CURLcode (*)(CURL *, CURLINFO, ...));
KATANA_DECLARED(curl_easy_cleanup, void (*)(CURL *));
KATANA_DECLARED(curl_easy_strerror, const char *(*)(CURLcode));
KATANA_DECLARED(curl_slist_append, struct curl_slist *(*)(struct curl_slist *, const char *));
KATANA_DECLARED(curl_slist_free_all, void (*)(struct curl_slist *));
