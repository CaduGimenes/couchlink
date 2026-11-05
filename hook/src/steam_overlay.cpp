// Shift+Tab for Steam's overlay in a copy on real Steam (the overlay is preloaded, see dllmain.cpp Preload).
//
// The overlay opens on Shift+Tab only from the real keyboard and only while the game is the window in front. During a
// session no copy is (the daemon keeps the focus on the desktop) and each player's keys come from the feed. So a
// Shift+Tab from the copy's own keyboard asks Steam for the overlay, as a game's own "open overlay" button does
// (ISteamFriends::ActivateGameOverlay), while the daemon brings the copy in front for the player to use it. Closing is
// Steam's own: Shift+Tab or Esc reach the overlay for real then.
//
// The interfaces come from steamclient itself in one fixed version (SteamClient017, SteamFriends017), which Steam
// keeps serving whatever version the game's steam_api was built with.
#include "common.h"

namespace cl {
namespace {

std::atomic<bool> g_open{false};
// Until then the copy's real keyboard and mouse reach it although it has a feed: the overlay reads only those.
std::atomic<ULONGLONG> g_real_until{0};

#ifdef _WIN64
const wchar_t kApi[] = L"steam_api64.dll";
const wchar_t kClient[] = L"steamclient64.dll";
using GetFriendsFn = void*(*)(void* self, int user, int pipe, const char* version);
using ActivateFn = void (*)(void* self, const char* dialog);
void* GetFriends(void* fn, void* self, int user, int pipe, const char* v) { return reinterpret_cast<GetFriendsFn>(fn)(self, user, pipe, v); }
void Activate(void* fn, void* self, const char* d) { reinterpret_cast<ActivateFn>(fn)(self, d); }
#else
const wchar_t kApi[] = L"steam_api.dll";
const wchar_t kClient[] = L"steamclient.dll";
// 32-bit Steam interfaces are __thiscall: `this` in ECX. __fastcall with an unused EDX argument passes it the same way.
using GetFriendsFn = void*(__fastcall*)(void* self, void* edx, int user, int pipe, const char* version);
using ActivateFn = void(__fastcall*)(void* self, void* edx, const char* dialog);
void* GetFriends(void* fn, void* self, int user, int pipe, const char* v) { return reinterpret_cast<GetFriendsFn>(fn)(self, nullptr, user, pipe, v); }
void Activate(void* fn, void* self, const char* d) { reinterpret_cast<ActivateFn>(fn)(self, nullptr, d); }
#endif

// Places in the interfaces' tables of functions (Steamworks SDK isteamclient.h, isteamfriends.h).
constexpr int kGetISteamFriends = 8;
constexpr int kActivateGameOverlay = 28;

DWORD WINAPI OpenOverlay(LPVOID) {
    HMODULE api = GetModuleHandleW(kApi);
    HMODULE client = GetModuleHandleW(kClient);
    if (!api || !client) {
        Note("Shift+Tab: no Steam in this copy");
        return 0;
    }
    auto user = reinterpret_cast<int (*)()>(GetProcAddress(api, "SteamAPI_GetHSteamUser"));
    auto pipe = reinterpret_cast<int (*)()>(GetProcAddress(api, "SteamAPI_GetHSteamPipe"));
    auto create = reinterpret_cast<void* (*)(const char*, int*)>(GetProcAddress(client, "CreateInterface"));
    if (!user || !pipe || !create || !user() || !pipe()) {
        Note("Shift+Tab: Steam is not set up in this copy yet");
        return 0;
    }
    void* steam = create("SteamClient017", nullptr);
    void* friends = steam ? GetFriends((*reinterpret_cast<void***>(steam))[kGetISteamFriends], steam, user(), pipe(), "SteamFriends017") : nullptr;
    if (!friends) {
        Note("Shift+Tab: Steam has no friends interface for this copy");
        return 0;
    }
    Activate((*reinterpret_cast<void***>(friends))[kActivateGameOverlay], friends, "Friends");
    Bump(C_OVERLAY_OPENED);
    return 0;
}

}  // namespace

bool SteamOverlayTakesRealInput() {
    return g_open.load() || GetTickCount64() < g_real_until.load();
}

void SteamOverlayKey(uint16_t vk, bool shift) {
    if (g_cfg.preload.empty()) return;
    if (vk == VK_ESCAPE) {
        if (g_open.exchange(false)) g_real_until = GetTickCount64() + 1500;
        return;
    }
    if (vk != VK_TAB || !shift) return;
    // Shift+Tab again closes it: the copy is in front by then, so the overlay takes that one from the keyboard itself.
    // The real keys keep reaching the copy a moment longer, since this feed event may come before them.
    if (g_open.exchange(!g_open.load())) {
        g_real_until = GetTickCount64() + 1500;
        return;
    }
    HANDLE t = CreateThread(nullptr, 0, OpenOverlay, nullptr, 0, nullptr);
    if (t) CloseHandle(t);
}

}  // namespace cl
