// The credential stores of include/katana/update/credential.hpp: the Windows Credential Manager
// (src/katana_update/credential_win.cpp), the environment variable (credential_env.cpp), and
// redactToken.
//
// The Credential Manager cases run against the REAL one, under a target name made unique for the
// process ("Katana/update-token-test-<pid>-<tick>"), never the name a person's token is kept
// under. They read what the store wrote with raw CredReadW and write what the store reads with raw
// CredWriteW and with cmdkey, so the format is held to the system and not to the store's own idea
// of it. Whatever happens in a case, the credential is deleted afterwards (the fixture's
// destructor); a run that is killed cannot do that, so the first case of the next run deletes the
// leftovers of processes that have ended (sweepOrphanedTestCredentials). A machine whose
// Credential Manager cannot be used (a service session with no profile, a stripped image) SKIPS
// with the system's reason instead of failing.
//
// Expected values are worked out by hand: UTF-8 bytes from the code points (U+00E9 is C3 A9,
// U+65E5 is E6 97 A5, U+672C is E6 9C AC, U+1F511 is F0 9F 94 91), the 2560-byte limit by
// asking the system itself, redaction by counting the occurrences in a short string.

#include <gtest/gtest.h>

#include <cstddef>
#include <cstdlib>
#include <initializer_list>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/update/credential.hpp"

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <wincred.h>
#endif

using namespace katana;

namespace {

// "tok" + e acute + "n-" + two CJK characters + "-" + a key emoji, as the UTF-8 bytes worked out
// above: 3 + 2 + 2 + 6 + 1 + 4 = 18 bytes. Split so that no hex escape runs into the next
// character.
const std::string kNonAsciiToken = std::string("tok") + "\xC3\xA9" + "n-" + "\xE6\x97\xA5" +
                                   "\xE6\x9C\xAC" + "-" + "\xF0\x9F\x94\x91";

// A token that no message may contain: its own text is what the checks look for.
constexpr std::string_view kSecret = "SECRETVALUE";

// `size` bytes of a token that starts with kSecret, so a message that quotes any of it is caught.
std::string tokenOfSize(std::size_t size)
{
    std::string token;
    while (token.size() < size) {
        token += kSecret;
        token += "-";
    }
    token.resize(size);
    return token;
}

bool mentionsSecret(const core::Error& error)
{
    const std::string text = error.describe();
    return text.find(kSecret) != std::string::npos;
}

// The bytes a token must not hold: the C0 controls 0x00-0x1F and DEL 0x7F, listed from the ASCII
// table and not from the rule under test. 0x00 is left out when `withNul` is false (an environment
// variable ends at it).
std::string controlBytes(bool withNul)
{
    std::string bytes;
    for (int byte = withNul ? 0x00 : 0x01; byte <= 0x1F; ++byte) {
        bytes += static_cast<char>(byte);
    }
    bytes += static_cast<char>(0x7F);
    return bytes;
}

// The bytes a token may hold and the system does not trim: 0x21 '!' to 0x7E '~', every visible
// ASCII character (0x20 is a space, which a token may not hold inside it).
std::string visibleBytes()
{
    std::string bytes;
    for (int byte = 0x21; byte <= 0x7E; ++byte) {
        bytes += static_cast<char>(byte);
    }
    return bytes;
}

// The six bytes that are white space around a token and no part of one: space, then 0x09-0x0D
// (tab, line feed, vertical tab, form feed, carriage return).
constexpr std::string_view kBlanks = " \t\n\v\f\r";

// For a failure message inside a loop over bytes.
std::string byteName(char byte)
{
    return "byte " + std::to_string(static_cast<unsigned>(static_cast<unsigned char>(byte))) +
           " (decimal)";
}

// The UTF-8 bytes of a code point below U+10000, from the encoding's own table and not from the
// code under test: U+0080-U+07FF is 110xxxxx 10xxxxxx, U+0800-U+FFFF is 1110xxxx 10xxxxxx
// 10xxxxxx (U+00A0 is C2 A0, U+2003 is E2 80 83, U+FEFF is EF BB BF).
std::string utf8Of(char32_t codePoint)
{
    std::string bytes;
    if (codePoint < 0x800) {
        bytes += static_cast<char>(0xC0 | (codePoint >> 6));
        bytes += static_cast<char>(0x80 | (codePoint & 0x3F));
    } else {
        bytes += static_cast<char>(0xE0 | (codePoint >> 12));
        bytes += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F));
        bytes += static_cast<char>(0x80 | (codePoint & 0x3F));
    }
    return bytes;
}

// The code points beyond ASCII that are white space or invisible, and that a token copied from a
// web page, a chat or a PDF carries at its ends: Unicode's White_Space list (U+0085, U+00A0,
// U+1680, U+2000-U+200A, U+2028, U+2029, U+202F, U+205F, U+3000) and the zero-width and direction
// format characters (U+180E, U+200B-U+200F, U+202A-U+202E, U+2060-U+206F, U+FEFF), each range
// written out by its ends. (U+2065 is unassigned and U+206A-U+206F are deprecated format controls;
// they sit inside the one run from U+205F to U+206F.)
std::vector<char32_t> invisibleCodePoints()
{
    std::vector<char32_t> codePoints = {0x0085, 0x00A0, 0x1680, 0x180E, 0x3000, 0xFEFF};
    for (const auto& [first, last] : {std::pair<char32_t, char32_t>{0x2000, 0x200F},
                                      {0x2028, 0x202F},
                                      {0x205F, 0x206F}}) {
        for (char32_t codePoint = first; codePoint <= last; ++codePoint) {
            codePoints.push_back(codePoint);
        }
    }
    return codePoints;
}

// Visible characters next to the ends of those ranges, which a token may hold anywhere: the
// inverted comma U+00A1, the Canadian syllabics letter U+167F and the Ogham letter U+1681 beside
// U+1680, the hyphen U+2010 after U+200F, the hyphenation point U+2027 before U+2028, the per mille
// sign U+2030 after U+202F, the vertical four dots U+205E before U+205F, superscript zero U+2070
// after U+206F, the ideographic comma U+3001 after U+3000, the full-width exclamation mark U+FF01.
std::vector<char32_t> visibleNeighbours()
{
    return {0x00A1, 0x167F, 0x1681, 0x2010, 0x2027, 0x2030, 0x205E, 0x2070, 0x3001, 0xFF01};
}

std::string codePointName(char32_t codePoint)
{
    return "code point " + std::to_string(static_cast<unsigned>(codePoint)) + " (decimal)";
}

// ---- the environment ----------------------------------------------------------------------

// Sets KATANA_UPDATE_TOKEN for one test and puts back what was there (a developer's machine may
// have a real one) - set in this process, never in anything a test starts.
class TokenVariable {
  public:
    TokenVariable() : original_(core::environmentVariable(name())) {}
    ~TokenVariable() { put(original_); }
    TokenVariable(const TokenVariable&) = delete;
    TokenVariable& operator=(const TokenVariable&) = delete;

    static const char* name() { return "KATANA_UPDATE_TOKEN"; }

    // The value as UTF-8 text. An empty value removes the variable on both platforms.
    void set(const std::string& utf8) const { put(utf8); }
    void unset() const { put({}); }

  private:
    static void put(const std::string& utf8)
    {
#ifdef _WIN32
        // Wide, so that what the system holds is the text itself and not the code page's reading
        // of it (core::environmentVariable reads wide and hands back UTF-8).
        const std::wstring wide = core::pathFromUtf8(utf8).wstring();
        (void)_wputenv_s(L"KATANA_UPDATE_TOKEN", wide.c_str());
#else
        if (utf8.empty()) {
            (void)unsetenv(name());
        } else {
            (void)setenv(name(), utf8.c_str(), 1);
        }
#endif
    }

    std::string original_;
};

class EnvironmentCredentialStoreTest : public testing::Test {
  protected:
    TokenVariable variable_;
    std::unique_ptr<update::CredentialStore> store_ = update::makeEnvironmentCredentialStore();
};

} // namespace

TEST(CredentialConstants, TheVariableAndTheTargetAreTheNamesTheDocumentationPromises)
{
    EXPECT_EQ(update::kTokenEnvironmentVariable, "KATANA_UPDATE_TOKEN");
    EXPECT_EQ(update::kCredentialTarget, "Katana/update-token");
}

TEST_F(EnvironmentCredentialStoreTest, ReadsTheTokenFromTheVariable)
{
    variable_.set("ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD");
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD");
    EXPECT_EQ(store_->where(), "environment");
}

TEST_F(EnvironmentCredentialStoreTest, ReadsATokenWithNonAsciiCharactersExactly)
{
    variable_.set(kNonAsciiToken);
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, kNonAsciiToken);
}

TEST_F(EnvironmentCredentialStoreTest, DropsTheWhiteSpaceAShellLeavesAroundTheValue)
{
    // `export KATANA_UPDATE_TOKEN="$(cat token.txt)"` keeps no newline, but a hand-edited profile
    // or a CI secret often does: the token is what is between.
    variable_.set(" \tabc123 \r\n");
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");
}

TEST_F(EnvironmentCredentialStoreTest, AVariableThatIsUnsetEmptyOrOnlyWhiteSpaceIsNoToken)
{
    variable_.unset();
    auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::NotFound);

    variable_.set("");
    token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::NotFound);

    variable_.set("  \t ");
    token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::NotFound);
}

TEST_F(EnvironmentCredentialStoreTest, ALineBreakInsideTheValueIsRefusedNotSentInAHeader)
{
    // "abc\r\nX-Evil: 1" would end the Authorization header and begin another.
    variable_.set(std::string(kSecret) + "\r\nX-Evil: 1");
    const auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
    // It says which variable to correct.
    EXPECT_NE(token.error().message.find("KATANA_UPDATE_TOKEN"), std::string::npos);
}

TEST_F(EnvironmentCredentialStoreTest, ASpaceInsideTheValueIsRefusedAndSaysToLeaveTheWordOut)
{
    // "Bearer <token>" pasted whole would be sent as "Authorization: Bearer Bearer <token>": a 401
    // that names no cause. Refused here, with a sentence that does.
    variable_.set("Bearer " + std::string(kSecret));
    auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
    EXPECT_NE(token.error().message.find("KATANA_UPDATE_TOKEN"), std::string::npos);
    EXPECT_NE(token.error().message.find("space"), std::string::npos) << token.error().describe();
    EXPECT_NE(token.error().message.find("Bearer"), std::string::npos) << token.error().describe();

    // A space in the middle of a longer value is the same; one at the ends is trimmed (above).
    variable_.set(std::string(kSecret) + " " + std::string(kSecret));
    token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
}

TEST_F(EnvironmentCredentialStoreTest, EveryControlCharacterInsideTheValueIsRefused)
{
    // 0x01-0x1F and 0x7F, each between two letters: an environment variable ends at 0x00, so that
    // one is the Credential Manager cases' (below). Every one would put a byte in an HTTP header
    // that it cannot carry, the line feed and the carriage return first. The boundaries are the
    // point: 0x1F and 0x0E-0x1B are not the "usual" line-break characters, and 0x7F is outside the
    // 0x00-0x1F run.
    for (const char byte : controlBytes(false)) {
        SCOPED_TRACE(byteName(byte));
        variable_.set(std::string(kSecret) + byte + "cd");
        const auto token = store_->read();
        ASSERT_FALSE(token);
        EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
        EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
    }
}

TEST_F(EnvironmentCredentialStoreTest, AControlCharacterAtAnEndIsRefusedUnlessItIsOneOfTheSixBlanks)
{
    for (const char byte : controlBytes(false)) {
        SCOPED_TRACE(byteName(byte));
        const bool blank = kBlanks.find(byte) != std::string_view::npos;
        for (const std::string& value :
             {byte + std::string(kSecret), std::string(kSecret) + byte,
              byte + std::string(kSecret) + byte}) {
            variable_.set(value);
            const auto token = store_->read();
            if (blank) {
                // Trimmed: the token is what is between.
                ASSERT_TRUE(token) << token.error().describe();
                EXPECT_EQ(*token, kSecret);
            } else {
                ASSERT_FALSE(token);
                EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
                EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
            }
        }
    }
}

TEST_F(EnvironmentCredentialStoreTest, EveryVisibleAsciiCharacterInsideTheValueIsAccepted)
{
    // 0x21 '!' to 0x7E '~': base64 and URL-safe alphabets, punctuation, a quote, a percent sign. A
    // rule that read "below 0x7F" wrongly as "below 0x7E", or ">= 0x21" as "> 0x21", fails here.
    for (const char byte : visibleBytes()) {
        SCOPED_TRACE(byteName(byte));
        const std::string value = std::string("ab") + byte + "cd";
        variable_.set(value);
        const auto token = store_->read();
        ASSERT_TRUE(token) << token.error().describe();
        EXPECT_EQ(*token, value);
    }
}

TEST_F(EnvironmentCredentialStoreTest, EachBlankAroundTheValueIsDroppedAndTheWordBetweenIsKept)
{
    // The six white-space bytes of the C locale, one at a time at both ends.
    for (const char blank : kBlanks) {
        SCOPED_TRACE(byteName(blank));
        variable_.set(blank + std::string("abc123") + blank);
        const auto token = store_->read();
        ASSERT_TRUE(token) << token.error().describe();
        EXPECT_EQ(*token, "abc123");
    }
}

TEST_F(EnvironmentCredentialStoreTest, EveryInvisibleOrNonAsciiBlankAroundTheValueIsDropped)
{
    // A no-break space, a zero-width space, a direction mark or a byte-order mark comes along with
    // a token copied from a web page, a chat or a PDF; core::trimmed knows only ASCII, and the
    // server would answer 401 with no cause.
    for (const char32_t codePoint : invisibleCodePoints()) {
        SCOPED_TRACE(codePointName(codePoint));
        const std::string blank = utf8Of(codePoint);
        variable_.set(blank + "abc123" + blank);
        const auto token = store_->read();
        ASSERT_TRUE(token) << token.error().describe();
        EXPECT_EQ(*token, "abc123");
    }
}

TEST_F(EnvironmentCredentialStoreTest, BlanksOfEveryKindAroundTheValueAreDroppedInAnyOrder)
{
    // Five at the front and four at the back, ASCII and not, interleaved: a pass of one kind would
    // leave the others. U+00A0 is C2 A0, U+FEFF is EF BB BF, U+200B is E2 80 8B.
    variable_.set(std::string("\xEF\xBB\xBF \t\xC2\xA0 abc123") + "\xE2\x80\x8B\r\n\xC2\xA0 ");
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");
}

TEST_F(EnvironmentCredentialStoreTest, AnInvisibleCharacterInsideTheValueIsRefusedLikeASpace)
{
    for (const char32_t codePoint : invisibleCodePoints()) {
        SCOPED_TRACE(codePointName(codePoint));
        variable_.set(std::string(kSecret) + utf8Of(codePoint) + "cd");
        const auto token = store_->read();
        ASSERT_FALSE(token);
        EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
        EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
        EXPECT_NE(token.error().message.find("KATANA_UPDATE_TOKEN"), std::string::npos);
        EXPECT_NE(token.error().message.find("invisible"), std::string::npos)
            << token.error().describe();
    }
}

TEST_F(EnvironmentCredentialStoreTest, TheCharactersNextToTheInvisibleOnesAreKeptEvenAtAnEnd)
{
    for (const char32_t codePoint : visibleNeighbours()) {
        SCOPED_TRACE(codePointName(codePoint));
        const std::string visible = utf8Of(codePoint);
        for (const std::string& value :
             {"ab" + visible + "cd", visible + "abc123", "abc123" + visible,
              visible + "abc123" + visible}) {
            variable_.set(value);
            const auto token = store_->read();
            ASSERT_TRUE(token) << token.error().describe();
            EXPECT_EQ(*token, value);
        }
    }
}

TEST_F(EnvironmentCredentialStoreTest, AValueOfNothingButInvisibleCharactersIsNotATokenAndSaysSo)
{
    // Not "unset": a person put something there, and the sentence names the variable.
    variable_.set(utf8Of(0x00A0) + utf8Of(0xFEFF) + " ");
    const auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_NE(token.error().message.find("KATANA_UPDATE_TOKEN"), std::string::npos);
}

#ifndef _WIN32
// (Windows holds a value as UTF-16, so a value that is not text cannot be set there.)
TEST_F(EnvironmentCredentialStoreTest, AValueThatIsNotUtf8IsRefused)
{
    // 0xC3 0x28 is a lead byte followed by a byte that is not a continuation.
    variable_.set(std::string(kSecret) + "\xC3\x28");
    const auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
}
#endif

TEST_F(EnvironmentCredentialStoreTest, ATokenOverTheSizeTheCredentialManagerHoldsIsRefused)
{
    variable_.set(tokenOfSize(2560));
    auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(token->size(), 2560U);

    variable_.set(tokenOfSize(2561));
    token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
}

TEST_F(EnvironmentCredentialStoreTest, WritingAndClearingAreUnsupportedAndSayWhyAndChangeNothing)
{
    variable_.set("abc123");

    const auto written = store_->write(kSecret);
    ASSERT_FALSE(written);
    EXPECT_EQ(written.error().code, core::ErrorCode::Unsupported);
    EXPECT_NE(written.error().message.find("KATANA_UPDATE_TOKEN"), std::string::npos);
    EXPECT_FALSE(mentionsSecret(written.error())) << written.error().describe();

    const auto cleared = store_->clear();
    ASSERT_FALSE(cleared);
    EXPECT_EQ(cleared.error().code, core::ErrorCode::Unsupported);
    EXPECT_NE(cleared.error().message.find("KATANA_UPDATE_TOKEN"), std::string::npos);

    // The variable is what it was: neither call touched it.
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");
}

#ifndef _WIN32
TEST_F(EnvironmentCredentialStoreTest, WhereThereIsNoKeyringTheSystemStoreIsTheEnvironment)
{
    variable_.set("abc123");
    const auto system = update::makeSystemCredentialStore();
    EXPECT_EQ(system->where(), "environment");
    const auto token = system->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");
}
#endif

// ---- redactToken --------------------------------------------------------------------------

TEST(RedactToken, ReplacesTheTokenInAHeaderLine)
{
    EXPECT_EQ(update::redactToken("Authorization: Bearer abc123", "abc123"),
              "Authorization: Bearer ***");
}

TEST(RedactToken, ReplacesEveryOccurrenceNotJustTheFirst)
{
    EXPECT_EQ(update::redactToken("abc-abc/abc", "abc"), "***-***/***");
}

TEST(RedactToken, TextWithoutTheTokenIsUnchanged)
{
    EXPECT_EQ(update::redactToken("no secret here", "abc123"), "no secret here");
    EXPECT_EQ(update::redactToken("", "abc123"), "");
}

TEST(RedactToken, AnEmptyTokenChangesNothingRatherThanMaskingBetweenEveryCharacter)
{
    EXPECT_EQ(update::redactToken("abc", ""), "abc");
    EXPECT_EQ(update::redactToken("", ""), "");
}

TEST(RedactToken, ATokenShorterThanFourCharactersIsStillReplaced)
{
    EXPECT_EQ(update::redactToken("xaxbx", "x"), "***a***b***");
    EXPECT_EQ(update::redactToken("pass: ab, ab", "ab"), "pass: ***, ***");
}

TEST(RedactToken, OccurrencesThatOverlapAreOneMaskSoNoPartOfTheTokenIsLeft)
{
    // "aaa" holds "aa" at 0 and at 1: covered together it is all three characters.
    EXPECT_EQ(update::redactToken("aaa", "aa"), "***");
    // "aaaa": at 0, 1 and 2, which together cover all four.
    EXPECT_EQ(update::redactToken("aaaa", "aa"), "***");
    // "ababab" and "abab": at 0 and at 2, covering 6 characters.
    EXPECT_EQ(update::redactToken("ababab", "abab"), "***");
    EXPECT_EQ(update::redactToken("x ababab y", "abab"), "x *** y");
}

TEST(RedactToken, OccurrencesThatOnlyTouchAreTwoMasks)
{
    EXPECT_EQ(update::redactToken("tokentoken", "token"), "******");
    EXPECT_EQ(update::redactToken("[token][token]", "token"), "[***][***]");
}

TEST(RedactToken, ATokenThatIsTheWholeTextIsMasked)
{
    EXPECT_EQ(update::redactToken("abc123", "abc123"), "***");
}

TEST(RedactToken, ATokenLongerThanTheTextCannotBeInIt)
{
    EXPECT_EQ(update::redactToken("abc", "abc123"), "abc");
}

TEST(RedactToken, MatchesTheTokenLiterallyAndCaseSensitively)
{
    // '.', '*' and '+' are characters of this token, not a pattern.
    EXPECT_EQ(update::redactToken("x a.b*c+ y aXb c", "a.b*c+"), "x *** y aXb c");
    EXPECT_EQ(update::redactToken("ABC", "abc"), "ABC");
}

TEST(RedactToken, ReplacesATokenWithNonAsciiCharacters)
{
    EXPECT_EQ(update::redactToken("Bearer " + kNonAsciiToken + " sent", kNonAsciiToken),
              "Bearer *** sent");
}

TEST(RedactToken, ReplacesTheTokenWhereverItIsInAUrlToo)
{
    EXPECT_EQ(update::redactToken("https://api.github.com/x?access_token=abc123&y=1", "abc123"),
              "https://api.github.com/x?access_token=***&y=1");
}

#ifdef _WIN32

namespace katana::update::detail {
// credential_win.cpp: seams for this test, not in the header, which is the contract for the one
// target a person's token is under. The Credential Manager store under another target name, and
// the same with every system call failing with a given error; what a Win32 error code means to a
// caller; the wide target name a store passes to the system.
std::unique_ptr<CredentialStore> makeCredentialManagerStore(std::string_view target);
std::unique_ptr<CredentialStore> makeCredentialManagerStoreFailingWith(std::string_view target,
                                                                       unsigned long code);
core::Error credentialError(DWORD code, std::string_view action, const std::string& target);
std::wstring credentialTargetOf(const CredentialStore& store);
} // namespace katana::update::detail

namespace {

// What the system itself holds under a target, read with the Win32 call and not through the store.
struct RawCredential {
    bool found = false;
    DWORD error = 0;
    DWORD type = 0;
    DWORD persist = 0;
    std::string blob;
    std::wstring targetName;
};

RawCredential rawRead(const std::wstring& target)
{
    RawCredential raw;
    PCREDENTIALW credential = nullptr;
    if (CredReadW(target.c_str(), CRED_TYPE_GENERIC, 0, &credential) == FALSE) {
        raw.error = GetLastError();
        return raw;
    }
    raw.found = true;
    raw.type = credential->Type;
    raw.persist = credential->Persist;
    raw.blob.assign(reinterpret_cast<const char*>(credential->CredentialBlob),
                    credential->CredentialBlobSize);
    raw.targetName = credential->TargetName;
    CredFree(credential);
    return raw;
}

// Stores `blob` exactly, as another program would. Returns the system's error, 0 on success.
DWORD rawWrite(const std::wstring& target, const std::string& blob)
{
    std::string bytes = blob;
    std::wstring name = target;
    CREDENTIALW credential{};
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = name.data();
    credential.CredentialBlobSize = static_cast<DWORD>(bytes.size());
    credential.CredentialBlob = reinterpret_cast<LPBYTE>(bytes.data());
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
    credential.UserName = const_cast<LPWSTR>(L"test");
    return CredWriteW(&credential, 0) != FALSE ? 0 : GetLastError();
}

// The bytes of UTF-16LE text from its code units, for a blob another tool would write.
std::string utf16le(std::initializer_list<unsigned> units)
{
    std::string bytes;
    for (const unsigned unit : units) {
        bytes += static_cast<char>(unit & 0xFF);
        bytes += static_cast<char>((unit >> 8) & 0xFF);
    }
    return bytes;
}

// The prefix of every target name these cases make. The names end "-<process id>-<tick>".
constexpr const wchar_t* kTestTargetPrefix = L"Katana/update-token-test-";

// Whether the process named in a test target ("...-<process id>-<tick>") may still be running.
// A name that does not have that shape, a process this user may not open, and this process itself
// all answer yes: only a credential that is certainly an orphan is deleted.
bool ownerMayBeRunning(const std::wstring& target)
{
    const std::size_t lastDash = target.rfind(L'-');
    if (lastDash == std::wstring::npos || lastDash == 0) {
        return true;
    }
    const std::size_t dashBefore = target.rfind(L'-', lastDash - 1);
    if (dashBefore == std::wstring::npos) {
        return true;
    }
    const std::wstring digits = target.substr(dashBefore + 1, lastDash - dashBefore - 1);
    if (digits.empty() || digits.size() > 9 || digits.find_first_not_of(L"0123456789") !=
                                                   std::wstring::npos) {
        return true;
    }
    const DWORD process = static_cast<DWORD>(std::stoul(digits));
    if (process == GetCurrentProcessId()) {
        return true;
    }
    HANDLE handle = OpenProcess(SYNCHRONIZE, FALSE, process);
    if (handle == nullptr) {
        // Only "there is no such process" proves it is gone; "access denied" proves it is there.
        return GetLastError() != ERROR_INVALID_PARAMETER;
    }
    const bool running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
    CloseHandle(handle);
    return running;
}

// Deletes the credentials that a run of this binary left behind because it was killed or crashed
// (a destructor cannot run then): the ones under kTestTargetPrefix whose process has ended. Never
// touches the target a person's token is under, which does not start with the prefix.
void sweepOrphanedTestCredentials()
{
    DWORD count = 0;
    PCREDENTIALW* found = nullptr;
    const std::wstring filter = std::wstring(kTestTargetPrefix) + L"*";
    if (CredEnumerateW(filter.c_str(), 0, &count, &found) == FALSE) {
        return; // none, or the Credential Manager cannot be used (the probe in SetUp skips then)
    }
    std::vector<std::wstring> orphans;
    for (DWORD i = 0; i < count; ++i) {
        const std::wstring target = found[i]->TargetName;
        if (found[i]->Type == CRED_TYPE_GENERIC && !ownerMayBeRunning(target)) {
            orphans.push_back(target);
        }
    }
    CredFree(found);
    for (const std::wstring& target : orphans) {
        (void)CredDeleteW(target.c_str(), CRED_TYPE_GENERIC, 0);
    }
}

// One case at a time in the whole session, whatever the number of processes. ctest runs each case
// as a process of its own, in parallel, and the Credential Manager does not keep reads and writes
// from different processes in order: measured on Windows 11 on 2026-10-08, with six processes
// writing and reading unrelated targets, a CredReadW returned the previous value of a target the
// same process had just written, a credential just written was not found, and a deleted one came
// back. Nothing a program does in one process can cause or fix that, and a person's Katana writes
// the token once, so only the test needs the order. A named mutex in the session's namespace
// (Local\) is released by the system if a process holding it is killed.
class CredentialManagerLock {
  public:
    CredentialManagerLock()
        : mutex_(CreateMutexW(nullptr, FALSE, L"Local\\KatanaUpdateCredentialTests"))
    {
    }
    ~CredentialManagerLock()
    {
        release();
        if (mutex_ != nullptr) {
            CloseHandle(mutex_);
        }
    }
    CredentialManagerLock(const CredentialManagerLock&) = delete;
    CredentialManagerLock& operator=(const CredentialManagerLock&) = delete;

    // Waits for the cases of the other processes; false when the lock cannot be had in `waitMs`.
    // A process that was killed while it held the lock leaves it "abandoned", which grants it.
    [[nodiscard]] bool acquire(DWORD waitMs)
    {
        if (mutex_ == nullptr) {
            return false;
        }
        const DWORD result = WaitForSingleObject(mutex_, waitMs);
        held_ = result == WAIT_OBJECT_0 || result == WAIT_ABANDONED;
        return held_;
    }

    void release()
    {
        if (held_) {
            (void)ReleaseMutex(mutex_);
            held_ = false;
        }
    }

  private:
    HANDLE mutex_;
    bool held_ = false;
};

class CredentialManagerStoreTest : public testing::Test {
  protected:
    void SetUp() override
    {
        // Ten minutes: the longest case takes seconds, and the whole suite is some forty of them.
        ASSERT_TRUE(lock_.acquire(600000))
            << "could not get the lock that keeps the cases from running at the same time";
        static const bool swept = (sweepOrphanedTestCredentials(), true);
        (void)swept;
        // The wide name is built from its own literals and digits, not converted from the narrow
        // one by the code under test or by anything like it: the raw calls below are held to
        // the name as the system sees it.
        const unsigned long long process = GetCurrentProcessId();
        const unsigned long long tick = GetTickCount64();
        targetUtf8_ = "Katana/update-token-test-" + std::to_string(process) + "-" +
                      std::to_string(tick);
        target_ = std::wstring(kTestTargetPrefix) + std::to_wstring(process) + L"-" +
                  std::to_wstring(tick);
        store_ = update::detail::makeCredentialManagerStore(targetUtf8_);
        // Nothing may be there before: the name is unique, and a leftover would make a case pass
        // on another case's token.
        ASSERT_FALSE(rawRead(target_).found);
        // Can this session use the Credential Manager at all? A write that is refused as
        // "not available" skips every case with the system's reason; any other failure is real.
        const core::Status probe = store_->write("probe");
        if (!probe) {
            if (probe.error().code == core::ErrorCode::Unsupported) {
                GTEST_SKIP() << "the Credential Manager cannot be used here: "
                             << probe.error().describe();
            }
            FAIL() << "probe write failed: " << probe.error().describe();
        }
        ASSERT_TRUE(store_->clear());
    }

    // Runs after a failed case too, and after a skip.
    void TearDown() override { cleanUp(); }
    ~CredentialManagerStoreTest() override { cleanUp(); }

    void cleanUp()
    {
        if (!target_.empty()) {
            (void)CredDeleteW(target_.c_str(), CRED_TYPE_GENERIC, 0);
        }
    }

    // First, so that it is released last: after cleanUp() has deleted the credential.
    CredentialManagerLock lock_;
    std::string targetUtf8_;
    std::wstring target_;
    std::unique_ptr<update::CredentialStore> store_;
};

} // namespace

TEST_F(CredentialManagerStoreTest, ReadBeforeAnythingWasWrittenIsNotFound)
{
    const auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::NotFound);
}

TEST_F(CredentialManagerStoreTest, ATokenWrittenIsReadBackEqual)
{
    ASSERT_TRUE(store_->write("ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD"));
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD");
    EXPECT_EQ(store_->where(), "credential manager");
}

TEST_F(CredentialManagerStoreTest, TheSystemHoldsAGenericLocalMachineCredentialOfTheUtf8Bytes)
{
    ASSERT_TRUE(store_->write("ghp_abc"));
    const RawCredential raw = rawRead(target_);
    ASSERT_TRUE(raw.found) << raw.error;
    EXPECT_EQ(raw.type, static_cast<DWORD>(CRED_TYPE_GENERIC));
    EXPECT_EQ(raw.persist, static_cast<DWORD>(CRED_PERSIST_LOCAL_MACHINE));
    EXPECT_EQ(raw.targetName, target_);
    // The seven ASCII bytes, with no terminator and no UTF-16 padding.
    EXPECT_EQ(raw.blob, "ghp_abc");
}

TEST_F(CredentialManagerStoreTest, ATokenWithNonAsciiCharactersIsReadBackEqualAndStoredAsItsUtf8)
{
    ASSERT_EQ(kNonAsciiToken.size(), 18U);
    ASSERT_TRUE(store_->write(kNonAsciiToken));
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, kNonAsciiToken);
    const RawCredential raw = rawRead(target_);
    ASSERT_TRUE(raw.found) << raw.error;
    EXPECT_EQ(raw.blob, kNonAsciiToken);
}

TEST_F(CredentialManagerStoreTest, WritingAgainReplacesTheTokenAndLeavesOneCredential)
{
    ASSERT_TRUE(store_->write("first-token"));
    ASSERT_TRUE(store_->write("second-token"));
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "second-token");
    EXPECT_EQ(rawRead(target_).blob, "second-token");
    // One clear removes it: there was never a second.
    ASSERT_TRUE(store_->clear());
    EXPECT_FALSE(rawRead(target_).found);
}

TEST_F(CredentialManagerStoreTest, ClearRemovesTheTokenSoReadAfterwardsIsNotFound)
{
    ASSERT_TRUE(store_->write("abc123"));
    ASSERT_TRUE(store_->clear());

    const auto token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::NotFound);

    const RawCredential raw = rawRead(target_);
    EXPECT_FALSE(raw.found);
    EXPECT_EQ(raw.error, static_cast<DWORD>(ERROR_NOT_FOUND));
}

TEST_F(CredentialManagerStoreTest, ClearingWhenNothingIsStoredSucceeds)
{
    EXPECT_TRUE(store_->clear());
    EXPECT_TRUE(store_->clear());
}

TEST_F(CredentialManagerStoreTest, WhiteSpaceAroundATokenIsNotStored)
{
    ASSERT_TRUE(store_->write("  abc123\r\n"));
    EXPECT_EQ(rawRead(target_).blob, "abc123");
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");
}

TEST_F(CredentialManagerStoreTest, ATokenOfExactlyTheLimitIsStoredAndOneByteMoreIsRefused)
{
    // The limit is the system's, not the store's idea of it: the same two sizes are put to the
    // Credential Manager directly, so a system that moved the limit fails here, not in the field.
    const std::string atLimit = tokenOfSize(2560);
    const std::string overLimit = tokenOfSize(2561);
    ASSERT_EQ(rawWrite(target_, atLimit), 0U);
    EXPECT_NE(rawWrite(target_, overLimit), 0U);
    ASSERT_TRUE(store_->clear());

    ASSERT_TRUE(store_->write(atLimit));
    auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, atLimit);

    const core::Status refused = store_->write(overLimit);
    ASSERT_FALSE(refused);
    EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("2561"), std::string::npos)
        << refused.error().describe();
    EXPECT_FALSE(mentionsSecret(refused.error())) << refused.error().describe();

    // The refused write changed nothing: the token at the limit is still there.
    token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, atLimit);
}

TEST_F(CredentialManagerStoreTest, ATokenThatCannotBeUsedInAHeaderIsRefusedAndNothingIsStored)
{
    const std::string withNul = std::string(kSecret) + std::string(1, '\0') + "x";
    const std::string notUtf8 = std::string(kSecret) + "\xC3\x28";
    const std::string cases[] = {
        "",
        " \t\r\n",
        std::string(kSecret) + "\n" + "X-Evil: 1",
        std::string(kSecret) + "\r\nX-Evil: 1",
        std::string(kSecret) + "\x01",
        std::string(kSecret) + "\x7F",
        withNul,
        notUtf8,
        // A word before the token, as "Bearer <token>" pasted whole; a space inside it; a tab.
        "Bearer " + std::string(kSecret),
        std::string(kSecret) + " " + std::string(kSecret),
        std::string(kSecret) + "\t" + std::string(kSecret),
    };
    for (const std::string& token : cases) {
        const core::Status refused = store_->write(token);
        ASSERT_FALSE(refused) << "accepted a token of " << token.size() << " bytes";
        EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_FALSE(mentionsSecret(refused.error())) << refused.error().describe();
        EXPECT_FALSE(rawRead(target_).found);
    }
}

TEST_F(CredentialManagerStoreTest, ASpaceInsideATokenIsRefusedAndSaysToLeaveTheWordOut)
{
    const core::Status refused = store_->write("Bearer " + std::string(kSecret));
    ASSERT_FALSE(refused);
    EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("space"), std::string::npos)
        << refused.error().describe();
    EXPECT_NE(refused.error().message.find("Bearer"), std::string::npos)
        << refused.error().describe();
    EXPECT_FALSE(mentionsSecret(refused.error())) << refused.error().describe();

    // Only the white space AROUND a token is dropped (above): "  abc123  " is "abc123".
    ASSERT_TRUE(store_->write("  abc123  "));
    EXPECT_EQ(rawRead(target_).blob, "abc123");
}

TEST_F(CredentialManagerStoreTest, InvisibleCharactersAroundATokenAreNotStoredAndInsideItAreRefused)
{
    // A token copied from a web page: a no-break space (U+00A0, C2 A0) in front and a byte-order
    // mark (U+FEFF, EF BB BF) behind are dropped, as ASCII blanks are.
    ASSERT_TRUE(store_->write(utf8Of(0x00A0) + "abc123" + utf8Of(0xFEFF)));
    EXPECT_EQ(rawRead(target_).blob, "abc123");
    ASSERT_TRUE(store_->clear());

    // Inside, every one of them is refused with the sentence that names the cause, and nothing is
    // stored.
    for (const char32_t codePoint : invisibleCodePoints()) {
        SCOPED_TRACE(codePointName(codePoint));
        const core::Status refused = store_->write(std::string(kSecret) + utf8Of(codePoint) + "cd");
        ASSERT_FALSE(refused);
        EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_NE(refused.error().message.find("invisible"), std::string::npos)
            << refused.error().describe();
        EXPECT_FALSE(mentionsSecret(refused.error())) << refused.error().describe();
        EXPECT_FALSE(rawRead(target_).found);
    }

    // The visible characters beside them are stored and read back, inside and at the ends.
    for (const char32_t codePoint : visibleNeighbours()) {
        SCOPED_TRACE(codePointName(codePoint));
        const std::string value = utf8Of(codePoint) + "abc" + utf8Of(codePoint);
        ASSERT_TRUE(store_->write(value));
        EXPECT_EQ(rawRead(target_).blob, value);
        const auto token = store_->read();
        ASSERT_TRUE(token) << token.error().describe();
        EXPECT_EQ(*token, value);
    }
}

TEST_F(CredentialManagerStoreTest, AStoredBlobWithInvisibleCharactersAtItsEndsIsReadWithoutThem)
{
    // A file saved as "UTF-8" by an editor that adds a byte-order mark, a token pasted with a
    // no-break space after it; stored by another program, so the blob is exactly this.
    ASSERT_EQ(rawWrite(target_, utf8Of(0xFEFF) + "abc123"), 0U);
    auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");

    ASSERT_EQ(rawWrite(target_, "abc123" + utf8Of(0x00A0)), 0U);
    token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");

    // Inside, it is not a token.
    ASSERT_EQ(rawWrite(target_, std::string(kSecret) + utf8Of(0x00A0) + "x"), 0U);
    token = store_->read();
    ASSERT_FALSE(token);
    EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
    EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
}

TEST_F(CredentialManagerStoreTest, ATruncatedInvisibleCharacterIsNotUtf8AndIsRefused)
{
    // The first one or two bytes of U+2003 (E2 80 83) and of U+FEFF (EF BB BF), at the front, at
    // the back, and alone: the rule that looks for these characters must find the end of the text
    // and stop, and the text is not UTF-8.
    const std::string truncated[] = {
        "\xE2",         "\xE2\x80",       "\xEF",         "\xEF\xBB",
        "abc\xE2\x80",  "\xE2\x80" "abc", "abc\xEF\xBB",  "\xEF\xBB" "abc",
    };
    for (const std::string& value : truncated) {
        const core::Status refused = store_->write(value);
        ASSERT_FALSE(refused) << "accepted a token of " << value.size() << " bytes";
        EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_FALSE(rawRead(target_).found);
    }
}

TEST_F(CredentialManagerStoreTest, ATokenAnotherToolStoredAsUtf16IsReadAsText)
{
    // cmdkey stores the password "abc" as 61 00 62 00 63 00 (measured on Windows 11, 2026-10-08).
    ASSERT_EQ(rawWrite(target_, utf16le({0x61, 0x62, 0x63})), 0U);
    auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc");

    // With a terminator, as some tools write: not part of the token.
    ASSERT_EQ(rawWrite(target_, utf16le({0x61, 0x62, 0x63, 0x00})), 0U);
    token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc");

    // 'a', U+00E9, U+65E5: the UTF-8 of the last two is C3 A9 and E6 97 A5.
    ASSERT_EQ(rawWrite(target_, utf16le({0x61, 0xE9, 0x65E5})), 0U);
    token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, std::string("a") + "\xC3\xA9" + "\xE6\x97\xA5");

    // U+1F511 is the surrogate pair D83D DD11 in UTF-16, F0 9F 94 91 in UTF-8.
    ASSERT_EQ(rawWrite(target_, utf16le({0x61, 0xD83D, 0xDD11})), 0U);
    token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, std::string("a") + "\xF0\x9F\x94\x91");
}

TEST_F(CredentialManagerStoreTest, ATokenAnotherToolStoredAsAUtf8CStringIsReadWithoutItsTerminator)
{
    ASSERT_EQ(rawWrite(target_, std::string("abc123") + std::string(1, '\0')), 0U);
    auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");

    // The same bytes with two terminators.
    ASSERT_EQ(rawWrite(target_, std::string("abc123") + std::string(2, '\0')), 0U);
    token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc123");
}

TEST_F(CredentialManagerStoreTest, ATokenMadeWithCmdkeyIsRead)
{
    // The real tool, as the independent writer. Its own failure (not on this machine, refused)
    // is a skip: this case is about reading what it wrote.
    const std::string command =
        "cmdkey /generic:\"" + targetUtf8_ + "\" /user:u /pass:abc > NUL 2>&1";
    if (std::system(command.c_str()) != 0) {
        GTEST_SKIP() << "cmdkey could not store a credential here";
    }
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "abc");
}

TEST_F(CredentialManagerStoreTest, WritingOverATokenMadeWithCmdkeyReplacesItWithTheUtf8Token)
{
    // The sequence a person follows: the documented cmdkey command, and later the Settings page
    // (write) with another token. The credential cmdkey made is replaced, not added to, and what
    // is left is the store's own form, the UTF-8 bytes, not cmdkey's UTF-16.
    const std::string command =
        "cmdkey /generic:\"" + targetUtf8_ + "\" /user:u /pass:abc > NUL 2>&1";
    if (std::system(command.c_str()) != 0) {
        GTEST_SKIP() << "cmdkey could not store a credential here";
    }
    // The premise, measured on Windows 11 on 2026-10-08: "abc" is 61 00 62 00 63 00.
    const RawCredential before = rawRead(target_);
    ASSERT_TRUE(before.found) << before.error;
    EXPECT_EQ(before.blob, utf16le({0x61, 0x62, 0x63}));

    ASSERT_TRUE(store_->write("new-token"));
    const RawCredential after = rawRead(target_);
    ASSERT_TRUE(after.found) << after.error;
    EXPECT_EQ(after.blob, "new-token"); // 9 bytes, no NUL
    EXPECT_EQ(after.type, static_cast<DWORD>(CRED_TYPE_GENERIC));
    EXPECT_EQ(after.persist, static_cast<DWORD>(CRED_PERSIST_LOCAL_MACHINE));
    const auto token = store_->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "new-token");

    // One credential under the name: a clear leaves nothing.
    ASSERT_TRUE(store_->clear());
    EXPECT_FALSE(rawRead(target_).found);
}

TEST_F(CredentialManagerStoreTest, EveryControlCharacterInsideATokenIsRefusedAndNothingIsStored)
{
    // 0x00-0x1F and 0x7F between a secret and a letter. Each is refused for the same reason (an
    // HTTP header cannot carry it), the line feed and the carriage return being the dangerous
    // ones; the boundaries 0x1F and 0x7F are the ones a rule written with < or <= gets wrong.
    for (const char byte : controlBytes(true)) {
        SCOPED_TRACE(byteName(byte));
        const core::Status refused = store_->write(std::string(kSecret) + byte + "x");
        ASSERT_FALSE(refused);
        EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_FALSE(mentionsSecret(refused.error())) << refused.error().describe();
        EXPECT_FALSE(rawRead(target_).found);
    }
}

TEST_F(CredentialManagerStoreTest, AControlCharacterAtAnEndIsRefusedUnlessItIsOneOfTheSixBlanks)
{
    for (const char byte : controlBytes(true)) {
        SCOPED_TRACE(byteName(byte));
        const bool blank = kBlanks.find(byte) != std::string_view::npos;
        for (const std::string& value :
             {byte + std::string(kSecret), std::string(kSecret) + byte,
              byte + std::string(kSecret) + byte}) {
            const core::Status written = store_->write(value);
            if (blank) {
                // Trimmed: what the system keeps is the token alone.
                ASSERT_TRUE(written) << written.error().describe();
                EXPECT_EQ(rawRead(target_).blob, kSecret);
                ASSERT_TRUE(store_->clear());
            } else {
                ASSERT_FALSE(written);
                EXPECT_EQ(written.error().code, core::ErrorCode::InvalidArgument);
                EXPECT_FALSE(mentionsSecret(written.error())) << written.error().describe();
                EXPECT_FALSE(rawRead(target_).found);
            }
        }
    }
}

TEST_F(CredentialManagerStoreTest, EveryVisibleAsciiCharacterInsideATokenIsStoredAndReadBack)
{
    for (const char byte : visibleBytes()) {
        SCOPED_TRACE(byteName(byte));
        const std::string value = std::string("ab") + byte + "cd";
        ASSERT_TRUE(store_->write(value));
        EXPECT_EQ(rawRead(target_).blob, value);
        const auto token = store_->read();
        ASSERT_TRUE(token) << token.error().describe();
        EXPECT_EQ(*token, value);
    }
}

TEST_F(CredentialManagerStoreTest, AStoredControlCharacterIsRefusedOnReadToo)
{
    // Another program stored it, or the machine was set up before this rule. 0x01-0x1F and 0x7F
    // between a secret and a letter; 0x00 is the UTF-16 and terminator cases' (above and below).
    for (const char byte : controlBytes(false)) {
        SCOPED_TRACE(byteName(byte));
        ASSERT_EQ(rawWrite(target_, std::string(kSecret) + byte + "x"), 0U);
        const auto token = store_->read();
        ASSERT_FALSE(token);
        EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
        EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
    }
    // The six blanks at the end of a stored token are not part of it.
    for (const char blank : kBlanks) {
        SCOPED_TRACE(byteName(blank));
        ASSERT_EQ(rawWrite(target_, std::string("abc123") + blank), 0U);
        const auto token = store_->read();
        ASSERT_TRUE(token) << token.error().describe();
        EXPECT_EQ(*token, "abc123");
    }
}

TEST_F(CredentialManagerStoreTest, TheSweepDeletesOnlyTheCredentialsOfProcessesThatHaveEnded)
{
    // Process ids are multiples of four and small; 999999996 is no process. 4 is the System
    // process, which a user cannot open: access denied proves it is there, so its credential stays.
    const std::wstring dead = std::wstring(kTestTargetPrefix) + L"999999996-1";
    const std::wstring deadNonAscii = std::wstring(kTestTargetPrefix) + L"\x00E9\x65E5-999999996-2";
    const std::wstring unopenable = std::wstring(kTestTargetPrefix) + L"4-1";
    const std::wstring thisProcess =
        std::wstring(kTestTargetPrefix) + std::to_wstring(GetCurrentProcessId()) + L"-1";
    const std::wstring noShape = std::wstring(kTestTargetPrefix) + L"abc";
    const std::wstring notOurs = L"Katana/update-token-testing-999999996-1";
    const std::vector<std::wstring> all = {dead,        deadNonAscii, unopenable,
                                           thisProcess, noShape,      notOurs};
    // Declared before anything is written, so it runs after a failure too.
    struct DeleteAllOnExit {
        const std::vector<std::wstring>& names;
        ~DeleteAllOnExit()
        {
            for (const std::wstring& name : names) {
                (void)CredDeleteW(name.c_str(), CRED_TYPE_GENERIC, 0);
            }
        }
    } const guard{all};
    for (const std::wstring& name : all) {
        ASSERT_EQ(rawWrite(name, "x"), 0U);
    }

    sweepOrphanedTestCredentials();

    EXPECT_FALSE(rawRead(dead).found);
    EXPECT_FALSE(rawRead(deadNonAscii).found);
    EXPECT_TRUE(rawRead(unopenable).found);
    EXPECT_TRUE(rawRead(thisProcess).found);
    EXPECT_TRUE(rawRead(noShape).found);
    EXPECT_TRUE(rawRead(notOurs).found);
}

TEST_F(CredentialManagerStoreTest, AStoredBlobThatIsNotATokenIsAnErrorThatDoesNotQuoteIt)
{
    // A control character in a UTF-8 blob, UTF-8 that is not well formed, an odd-length blob with
    // a NUL inside (neither UTF-8 nor UTF-16), a lone surrogate in UTF-16, and a space inside
    // ("Bearer <token>", in UTF-8 and in UTF-16: 'a' U+0020 'b').
    const std::string blobs[] = {
        std::string(kSecret) + "\x01" + "x",
        std::string(kSecret) + "\xC3\x28",
        std::string(kSecret) + std::string(1, '\0') + "x",
        utf16le({0x61, 0xD83D}),
        "Bearer " + std::string(kSecret),
        utf16le({0x61, 0x20, 0x62}),
    };
    for (const std::string& blob : blobs) {
        ASSERT_EQ(rawWrite(target_, blob), 0U);
        const auto token = store_->read();
        ASSERT_FALSE(token) << "read a token from a blob of " << blob.size() << " bytes";
        EXPECT_EQ(token.error().code, core::ErrorCode::InvalidState);
        EXPECT_FALSE(mentionsSecret(token.error())) << token.error().describe();
    }
}

TEST_F(CredentialManagerStoreTest, ACredentialWithNothingInItIsNoToken)
{
    // An empty password, or one of only terminators and white space, is a credential that exists
    // and holds no token: absent, not an error.
    const std::string blobs[] = {"", std::string(2, '\0'), " \r\n", utf16le({0x20, 0x0A})};
    for (const std::string& blob : blobs) {
        const DWORD written = rawWrite(target_, blob);
        if (written != 0) {
            // The system's rule, not the store's: an empty blob may not be kept at all.
            ASSERT_TRUE(blob.empty()) << "a blob of " << blob.size() << " bytes: error " << written;
            continue;
        }
        const auto token = store_->read();
        ASSERT_FALSE(token) << "read a token from a blob of " << blob.size() << " bytes";
        EXPECT_EQ(token.error().code, core::ErrorCode::NotFound);
    }
}

TEST_F(CredentialManagerStoreTest, ANonAsciiTargetReachesTheSystemAsTheCharactersItNames)
{
    // U+00E9 is C3 A9, U+65E5 is E6 97 A5, U+1F511 is F0 9F 94 91 in UTF-8, and the UTF-16 code
    // units E9, 65E5 and the surrogate pair D83D DD11. The store gets the UTF-8 bytes; the wide
    // name is written out as the code units the system must receive.
    // Under the test prefix, so that the sweep finds it if this process is killed.
    const std::string suffix =
        std::to_string(GetCurrentProcessId()) + "-" + std::to_string(GetTickCount64());
    const std::string utf8 = std::string("Katana/update-token-test-") + "\xC3\xA9" +
                             "\xE6\x97\xA5" + "\xF0\x9F\x94\x91" + "-" + suffix;
    const std::wstring wide = std::wstring(kTestTargetPrefix) +
                              L"\x00E9\x65E5\xD83D\xDD11-" +
                              std::wstring(suffix.begin(), suffix.end());
    // Declared before anything is written, so it runs after a failure too.
    struct DeleteOnExit {
        const std::wstring& name;
        ~DeleteOnExit() { (void)CredDeleteW(name.c_str(), CRED_TYPE_GENERIC, 0); }
    } const guard{wide};

    const auto store = update::detail::makeCredentialManagerStore(utf8);
    EXPECT_EQ(update::detail::credentialTargetOf(*store), wide);
    ASSERT_TRUE(store->write("tok-1"));
    const RawCredential raw = rawRead(wide);
    ASSERT_TRUE(raw.found) << raw.error;
    EXPECT_EQ(raw.targetName, wide);
    EXPECT_EQ(raw.blob, "tok-1");
    const auto token = store->read();
    ASSERT_TRUE(token) << token.error().describe();
    EXPECT_EQ(*token, "tok-1");
    ASSERT_TRUE(store->clear());
    EXPECT_FALSE(rawRead(wide).found);
}

TEST(CredentialManagerTarget, TheSystemStoreIsTheCredentialManagerAndATargetMustBeNamed)
{
    // Not read or written: the name a person's token is under is never touched by a test.
    EXPECT_EQ(update::makeSystemCredentialStore()->where(), "credential manager");
    EXPECT_THROW((void)update::detail::makeCredentialManagerStore(""), std::invalid_argument);
    EXPECT_THROW((void)update::detail::makeCredentialManagerStore("bad\xC3\x28"),
                 std::invalid_argument);
    // 32767 characters is the most the system takes as a generic target (CRED_MAX_GENERIC_TARGET_
    // NAME_LENGTH), and one more is refused before any call.
    EXPECT_NO_THROW((void)update::detail::makeCredentialManagerStore(std::string(32767, 'a')));
    EXPECT_THROW((void)update::detail::makeCredentialManagerStore(std::string(32768, 'a')),
                 std::invalid_argument);
}

TEST(CredentialManagerTarget, TheSystemStoreIsUnderTheNameTheDocumentationTellsAPersonToUse)
{
    // The literal, not kCredentialTarget: `cmdkey /generic:Katana/update-token /user:x /pass:...`
    // is what a person types, so the name the system store passes to the system is held to it.
    // Nothing is read or written under it.
    const auto system = update::makeSystemCredentialStore();
    EXPECT_EQ(update::detail::credentialTargetOf(*system), std::wstring(L"Katana/update-token"));

    // The seam reports the store's own name, not a fixed one, and none for another kind of store.
    const auto other = update::detail::makeCredentialManagerStore("Katana/other");
    EXPECT_EQ(update::detail::credentialTargetOf(*other), std::wstring(L"Katana/other"));
    const auto environment = update::makeEnvironmentCredentialStore();
    EXPECT_EQ(update::detail::credentialTargetOf(*environment), std::wstring());
}

TEST(CredentialManagerErrors, TheCodesForNoCredentialManagerInThisSessionAreUnsupported)
{
    // By number, from Microsoft's "System Error Codes" list, not by the macros the code uses.
    struct Code {
        DWORD number;
        const char* name;
    };
    const Code codes[] = {
        {5, "ERROR_ACCESS_DENIED"},
        {50, "ERROR_NOT_SUPPORTED"},
        {120, "ERROR_CALL_NOT_IMPLEMENTED"},
        {1312, "ERROR_NO_SUCH_LOGON_SESSION"},
        {1355, "ERROR_NO_SUCH_DOMAIN"},
    };
    for (const Code& code : codes) {
        const core::Error error = update::detail::credentialError(code.number, "read", "Katana/x");
        EXPECT_EQ(error.code, core::ErrorCode::Unsupported) << code.name;
        EXPECT_NE(error.message.find("not available in this session"), std::string::npos)
            << code.name << ": " << error.describe();
        // The number is there, so a refusal of access is not hidden by the sentence.
        EXPECT_NE(error.message.find("(Windows error " + std::to_string(code.number) + ")"),
                  std::string::npos)
            << code.name << ": " << error.describe();
        EXPECT_EQ(error.context, "Katana/x") << code.name;
    }
}

TEST(CredentialManagerFailures, EachOperationNamesItsOwnActionAndMapsTheCodeToItsKindOfError)
{
    // The system's three calls are replaced by calls that fail with a given code (nothing is read,
    // stored or removed), so that what read(), write() and clear() each make of the failure is held
    // to their own words: the mapping function alone (below) would pass if write() said "read".
    // 1312 is ERROR_NO_SUCH_LOGON_SESSION (no Credential Manager in this session), 87 is
    // ERROR_INVALID_PARAMETER (any other fault).
    const std::string token = std::string(kSecret) + "-1";
    {
        const auto store =
            update::detail::makeCredentialManagerStoreFailingWith("Katana/x", 1312);
        const auto looked = store->read();
        ASSERT_FALSE(looked);
        EXPECT_EQ(looked.error().code, core::ErrorCode::Unsupported);
        EXPECT_EQ(looked.error().context, "Katana/x");
        const core::Status written = store->write(token);
        ASSERT_FALSE(written);
        EXPECT_EQ(written.error().code, core::ErrorCode::Unsupported);
        EXPECT_EQ(written.error().context, "Katana/x");
        EXPECT_FALSE(mentionsSecret(written.error())) << written.error().describe();
        const core::Status cleared = store->clear();
        ASSERT_FALSE(cleared);
        EXPECT_EQ(cleared.error().code, core::ErrorCode::Unsupported);
        EXPECT_EQ(cleared.error().context, "Katana/x");
        for (const core::Error* error : {&looked.error(), &written.error(), &cleared.error()}) {
            EXPECT_NE(error->message.find("not available in this session (Windows error 1312)"),
                      std::string::npos)
                << error->describe();
        }
    }
    {
        const auto store = update::detail::makeCredentialManagerStoreFailingWith("Katana/x", 87);
        const auto looked = store->read();
        ASSERT_FALSE(looked);
        EXPECT_EQ(looked.error().code, core::ErrorCode::Internal);
        EXPECT_EQ(looked.error().context, "Katana/x");
        EXPECT_NE(looked.error().message.find("could not read the token (Windows error 87)"),
                  std::string::npos)
            << looked.error().describe();

        const core::Status written = store->write(token);
        ASSERT_FALSE(written);
        EXPECT_EQ(written.error().code, core::ErrorCode::Internal);
        EXPECT_EQ(written.error().context, "Katana/x");
        EXPECT_NE(written.error().message.find("could not store the token (Windows error 87)"),
                  std::string::npos)
            << written.error().describe();
        EXPECT_FALSE(mentionsSecret(written.error())) << written.error().describe();

        const core::Status cleared = store->clear();
        ASSERT_FALSE(cleared);
        EXPECT_EQ(cleared.error().code, core::ErrorCode::Internal);
        EXPECT_EQ(cleared.error().context, "Katana/x");
        EXPECT_NE(cleared.error().message.find("could not remove the token (Windows error 87)"),
                  std::string::npos)
            << cleared.error().describe();
    }
}

TEST(CredentialManagerFailures, NotFoundIsNoTokenForReadAndSuccessForClearButAFaultForWrite)
{
    // 1168 is ERROR_NOT_FOUND. Reading what is not there is the normal "none"; clearing it is the
    // state asked for; but storing cannot meet it, so a write that is told that is a fault.
    const auto store = update::detail::makeCredentialManagerStoreFailingWith("Katana/x", 1168);
    const auto looked = store->read();
    ASSERT_FALSE(looked);
    EXPECT_EQ(looked.error().code, core::ErrorCode::NotFound);
    EXPECT_TRUE(store->clear());
    const core::Status written = store->write("abc123");
    ASSERT_FALSE(written);
    EXPECT_EQ(written.error().code, core::ErrorCode::Internal);
    EXPECT_NE(written.error().message.find("(Windows error 1168)"), std::string::npos)
        << written.error().describe();
}

TEST(CredentialManagerFailures, AnUnusableTokenIsRefusedBeforeTheSystemIsAsked)
{
    // The refusal is the token's (InvalidArgument), not the system's (87): the rule runs first, so
    // a person is told what is wrong with the token even where the Credential Manager is down.
    const auto store = update::detail::makeCredentialManagerStoreFailingWith("Katana/x", 1312);
    const core::Status refused = store->write("Bearer " + std::string(kSecret));
    ASSERT_FALSE(refused);
    EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
}

TEST(CredentialManagerErrors, EveryOtherCodeIsAnInternalFaultThatNamesTheActionAndTheCode)
{
    // 1783 is what CredWriteW answers for a blob over the limit (measured on Windows 11,
    // 2026-10-08), 87 is ERROR_INVALID_PARAMETER, 1168 is ERROR_NOT_FOUND (read and clear take
    // that one before they ask), 2 is ERROR_FILE_NOT_FOUND; the others sit next to the codes
    // above, so that a rule that matched a range would be caught.
    const DWORD numbers[] = {2, 49, 51, 87, 119, 121, 1168, 1311, 1313, 1354, 1356, 1783};
    for (const DWORD number : numbers) {
        for (const char* action : {"read", "store", "remove"}) {
            const core::Error error = update::detail::credentialError(number, action, "Katana/x");
            EXPECT_EQ(error.code, core::ErrorCode::Internal) << number;
            EXPECT_NE(error.message.find(std::string("could not ") + action + " the token"),
                      std::string::npos)
                << number << ": " << error.describe();
            EXPECT_NE(error.message.find("(Windows error " + std::to_string(number) + ")"),
                      std::string::npos)
                << number << ": " << error.describe();
            EXPECT_EQ(error.context, "Katana/x") << number;
        }
    }
}

#endif // _WIN32
