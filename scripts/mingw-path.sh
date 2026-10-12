# Sourced by the build scripts: puts MinGW binutils (WinLibs via winget, or MSYS2) on PATH.
for d in "$LOCALAPPDATA"/Microsoft/WinGet/Packages/BrechtSanders.WinLibs*/mingw64/bin /c/msys64/mingw64/bin /c/msys64/ucrt64/bin; do
  if [ -x "$d/as.exe" ]; then export PATH="$d:$PATH"; break; fi
done
