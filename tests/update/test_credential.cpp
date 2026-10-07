// The credential stores of include/katana/update/credential.hpp: the Windows Credential Manager
// (src/katana_update/credential_win.cpp), the environment variable (credential_env.cpp), and
// redactToken.
//
// The Credential Manager cases run against the REAL one, under a target name made unique for the
// process ("Katana/update-token-test-<pid>-<tick>"), never the name a person's token is kept
// under. They read what the store wrote with raw CredReadW and write what the store reads with raw
// CredWriteW and with cmdkey, so the format is held to the system and not to the store's own idea
// of it. Whatever happens in a case, the credential is deleted afterwards (the fixture's
// destructor), and a machine whose Credential Manager cannot be used (a service session with no
// profile, a stripped image) SKIPS with the system's reason instead of failing.
//
// Expected values are worked out by hand: UTF-8 bytes from the code points (U+00E9 is C3 A9,
// U+65E5 is E6 97 A5, U+672C is E6 9C AC, U+1F511 is F0 9F 94 91), the 2560-byte limit by
// asking the system itself, redaction by counting the occurrences in a short string.

#include <gtest/gtest.h>

#include <cstdlib>
#include <initializer_list>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>

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
// credential_win.cpp: the Credential Manager store under another target name. Not in the header,
// which is the contract for the one target a person's token is under.
std::unique_ptr<CredentialStore> makeCredentialManagerStore(std::string_view target);
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

class CredentialManagerStoreTest : public testing::Test {
  protected:
    void SetUp() override
    {
        targetUtf8_ = "Katana/update-token-test-" + std::to_string(GetCurrentProcessId()) + "-" +
                      std::to_string(GetTickCount64());
        target_ = core::pathFromUtf8(targetUtf8_).wstring();
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
    };
    for (const std::string& token : cases) {
        const core::Status refused = store_->write(token);
        ASSERT_FALSE(refused) << "accepted a token of " << token.size() << " bytes";
        EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_FALSE(mentionsSecret(refused.error())) << refused.error().describe();
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

TEST_F(CredentialManagerStoreTest, AStoredBlobThatIsNotATokenIsAnErrorThatDoesNotQuoteIt)
{
    // A control character in a UTF-8 blob, UTF-8 that is not well formed, an odd-length blob with
    // a NUL inside (neither UTF-8 nor UTF-16), and a lone surrogate in UTF-16.
    const std::string blobs[] = {
        std::string(kSecret) + "\x01" + "x",
        std::string(kSecret) + "\xC3\x28",
        std::string(kSecret) + std::string(1, '\0') + "x",
        utf16le({0x61, 0xD83D}),
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

TEST(CredentialManagerTarget, TheSystemStoreIsTheCredentialManagerAndATargetMustBeNamed)
{
    // Not read or written: the name a person's token is under is never touched by a test.
    EXPECT_EQ(update::makeSystemCredentialStore()->where(), "credential manager");
    EXPECT_THROW((void)update::detail::makeCredentialManagerStore(""), std::invalid_argument);
    EXPECT_THROW((void)update::detail::makeCredentialManagerStore("bad\xC3\x28"),
                 std::invalid_argument);
}

#endif // _WIN32
