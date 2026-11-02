#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <windows.h>
#include <shellapi.h>

#include <string>

#include "flutter_window.h"
#include "utils.h"

// A launcher's start (`couchlink.exe --play <game>` or `--play-exe <program>`): couchlinkd next to this program
// starts the game's session, starting the daemon when needed and saying what went wrong in a message box.
static void PassPlayToDaemon() {
  int argc = 0;
  wchar_t** argv = ::CommandLineToArgvW(::GetCommandLineW(), &argc);
  if (argv == nullptr) return;
  std::wstring args;
  for (int i = 1; i + 1 < argc; ++i) {
    if (wcscmp(argv[i], L"--play") == 0 || wcscmp(argv[i], L"--play-exe") == 0) {
      std::wstring value = argv[i + 1];
      args = std::wstring(argv[i]) + L" \"" + value + L"\" --notify";
      break;
    }
  }
  ::LocalFree(argv);
  if (args.empty()) return;
  wchar_t self[MAX_PATH];
  DWORD n = ::GetModuleFileNameW(nullptr, self, MAX_PATH);
  if (n == 0 || n >= MAX_PATH) return;
  std::wstring dir(self, n);
  dir = dir.substr(0, dir.find_last_of(L"\\/") + 1);
  std::wstring line = L"\"" + dir + L"couchlinkd.exe\" " + args;
  STARTUPINFOW si = {sizeof(si)};
  PROCESS_INFORMATION pi = {};
  if (::CreateProcessW(nullptr, line.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW, nullptr, dir.c_str(), &si, &pi)) {
    ::CloseHandle(pi.hThread);
    ::CloseHandle(pi.hProcess);
  }
}

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  PassPlayToDaemon();
  // One Couchlink window: a second start brings the open one to the front instead.
  HANDLE single = ::CreateMutexW(nullptr, FALSE, L"Local\\CouchlinkApp");
  if (single != nullptr && ::GetLastError() == ERROR_ALREADY_EXISTS) {
    HWND open = ::FindWindowW(L"FLUTTER_RUNNER_WIN32_WINDOW", L"Couchlink");
    if (open != nullptr) {
      if (::IsIconic(open)) ::ShowWindow(open, SW_RESTORE);
      ::SetForegroundWindow(open);
    }
    return EXIT_SUCCESS;
  }

  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  flutter::DartProject project(L"data");

  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project);
  Win32Window::Point origin(10, 10);
  Win32Window::Size size(1280, 720);
  if (!window.Create(L"Couchlink", origin, size)) {
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  ::CoUninitialize();
  return EXIT_SUCCESS;
}
