# Участие в разработке

Спасибо, что заглянули! Проект небольшой, поэтому правила простые.

## Сборка

Нужны [Rust](https://rustup.rs) (stable, MSVC) и Visual Studio Build Tools с Windows SDK.

```powershell
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```

`.\tools\build.ps1` собирает релиз в `dist\` и перезапускает программу в трее.

## Перед pull request

- `cargo test` и `cargo clippy --all-targets -- -D warnings` проходят (их же запускает CI).
- Если меняли симуляцию или шейдер, проверьте нагрузку: `.\tools\bench.ps1` (процессор и память процесса оверлея) и `HZ_PROFILE=1` (время кадра в `%APPDATA%\Flurry\log.txt`). Цель — не больше ~0,5% ЦП в обычных режимах.
- Новые настройки добавляйте в `Settings` с разумным значением в `Default` и ограничением в `sanitize()`: старые `settings.json` должны читаться без ошибок.
- Интерфейс и тексты — на русском.
- Опишите изменение в начале `CHANGELOG.md`.

## Устройство

Карта модулей — в разделе «Как устроено» в [README](README.md). Коротко: основной процесс (трей, горячие клавиши, поток рендера на D3D11 + DirectComposition) и отдельный процесс окна настроек (egui), которые общаются через `settings.json` и `WM_APP`-сообщения скрытому окну.
