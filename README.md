# Rustlet

Samostatná aplikace **Rustlet** běží jako ikona vrstev v horní liště. První položkou jejího katalogu je **Desktop Tool Ring**. Ukazuje nainstalovanou verzi, skutečný stav procesu a dostupné vydání. Nástroj můžeš zapnout, vypnout, otevřít jeho nastavení a nainstalovat či aktualizovat podepsaný balíček. Nabídka v liště má tyto akce přímo u každého nástroje; **Otevřít správce…** zobrazí hlavní okno.

```sh
./scripts/install-manager.sh
# Volitelně jen pro aktuálního uživatele:
./scripts/install-manager.sh --user
```

Vznikne `/Applications/Rustlet.app` (nebo `~/Applications/Rustlet.app`). Správce převezme existující Desktop Tool Ring z Applications a zachová jeho předvolby i profily. Přepínač **Zapnuto** ovládá skutečný běh aplikace a uloží požadovaný stav. Při dalším startu správce obnoví zapnuté nástroje; vypnutý Desktop Tool Ring se nespustí ani přes starší položku přihlášení. **Spustit Rustlet při přihlášení** je samostatná volba přes `SMAppService`. Ukončení správce ponechá již běžící nástroje spuštěné. Spouštění při přihlášení se samo nezapíná.

Kontrola nových vydání probíhá na pozadí při startu a každou hodinu, lze ji vypnout nebo spustit ručně. Instalaci či aktualizaci spouští uživatel tlačítkem. Správce ověří Ed25519 podpis metadat i archivu, identitu aplikace, rostoucí číslo sestavení, architekturu, macOS, Developer ID a notarizaci. Teprve potom požádá běžící nástroj o ukončení, vymění balíček se zálohou a znovu ho spustí, pokud původně běžel. Nástroj, který odmítne ukončení kvůli rozpracované operaci, se násilně neukončuje. Chyba výměny vrátí původní balíček; při selhání obnovy se záloha zachová a její cesta je v chybě. Preference aplikace se při výměně nemažou.

Když Desktop Tool Ring převezme Rustlet, jeho samostatný Sparkle updater neprovádí další kontroly. Karta **Aktualizace** v nástroji odkáže na správce. Lokální příznak správy je oddělený od uživatelských profilů.

**Distribuce:** připravený cíl `DanielHromadka/desktop-tool-ring-releases` zatím nebyl zveřejněný ani naplněný. V takovém případě Rustlet zobrazí „Zatím není zveřejněné dostupné vydání“ a běh nainstalovaného nástroje funguje dál. Vytvoření veřejného distribučního repozitáře čeká na potvrzení vlastníka. Zdrojový repozitář zůstává soukromý. Pro online instalace potřebuješ podepsané/notarizované vydání publikované release skriptem v repozitáři Desktop Tool Ring.

### aFinder ve správci

Rustlet rozpozná **aFinder** (`cz.attosoft.afinder`) v `/Applications/aFinder.app` nebo `~/Applications/aFinder.app`. Můžeš ho zapnout, šetrně ukončit a otevřít tlačítkem **Otevřít…**. Zapnutý stav se obnoví při příštím spuštění Rustletu.

Pokud aFinder ještě nemáš, klikni v Rustletu na **Nainstalovat**. Správce stáhne nejnovější stabilní ZIP z veřejného `DanielHromadka/afinder-downloads` pro Apple Silicon nebo Intel, ověří velikost a SHA-256 z GitHub Release API, identitu aplikace a verzi, minimální macOS, podpis připnutého Developer ID a notarizaci přes Gatekeeper. Aplikaci nainstaluje do `/Applications` (nebo `~/Applications`, pokud do systémové složky nelze zapisovat), převezme ji do správy a otevře. Není potřeba ručně stahovat DMG ani přetahovat aplikaci. Stažený balíček se při chybě odstraní a existující instalace se touto cestou nikdy nepřepisuje.

Tlačítkem **Přidat aplikaci…** u nainstalovaného aFinderu vybereš jinou existující kopii i z jiného umístění nebo s jiným názvem. Když aFinder není rozpoznaný, je tato možnost v jeho nabídce v liště jako **Vybrat existující aplikaci…**. Správce ověří bundle ID a zapamatuje si cestu. Další aktualizace spravuje samotný aFinder svým updaterem nebo přes App Store; Rustlet mu neposílá přepínače určené pro Desktop Tool Ring.

### Přidání dalšího nástroje

Katalog je v `resources/catalog.json`. Záznam nástroje s aktualizacemi přes Rustlet obsahuje stabilní ID, bundle ID, jméno `.app`, popis, symbol, URL podepsaného `release.json`, povolený prefix archivu, veřejný Ed25519 klíč a Apple Team ID. Nový nástroj přidej jako další záznam a sestav novou verzi správce. UI a instalátor už pracují s více položkami katalogu. Aplikace používají `--background` pro spuštění bez nastavení a `--settings` pro jeho otevření. Další nástroje si vytvoří vlastní podpisový účet odpovídající bundle ID; pro převzetí aktualizací mohou použít stejný lokální protokol `managedByRustlet` / `RustletManagementChanged` jako Desktop Tool Ring.

Pro aplikaci s vlastním updaterem použij `integration: "external"`, `github_repository` a `team_id`. Veřejné stabilní GitHub vydání musí obsahovat `<název>-arm64-mac.zip` a `<název>-x64-mac.zip` s kontrolním součtem SHA-256 v GitHub API. ZIP obsahuje podepsaný a notarizovaný `.app` balíček. Rustlet provádí první instalaci, spouští aplikaci bez argumentů a dovolí vybrat její umístění; další aktualizace ponechá aplikaci. Ověření skutečného veřejného vydání do dočasné složky spustíš přes `cargo test -p rustlet afinder_download_install_end_to_end -- --ignored`.

`release.json` je podepsaná obálka s `payload` (base64 původních JSON bajtů) a `signature` (base64 Ed25519 podpisu). Payload obsahuje schéma, ID, bundle ID, verzi, číslo sestavení, minimální macOS, architektury a URL/velikost/podpis archivu. V repozitáři Desktop Tool Ring ho vytváří `scripts/manager-manifest.py`; soukromý klíč používá přímo z Keychain, neexportuje ho. Tamní `scripts/release.sh` publikuje vedle Sparkle appcastu také tuto obálku, takže jedno vydání podporuje správce i samostatný nástroj.


## Vývoj

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
./scripts/build-manager.sh
```

Samostatný repozitář extrahovaný z Desktop Tool Ring se zachovanou historií. Sestavení nevyžaduje zdrojové soubory spravovaných aplikací.

## macOS instalátory a GitHub Actions

Repozitář: https://github.com/DanielHromadka/rustlet

Workflow **macOS installers** běží po pushi do `main`, v pull requestech a ručně přes **Actions → macOS installers → Run workflow**. Oddělené macOS runnery sestaví Apple Silicon (`arm64`) a Intel (`x86_64`), spustí Rust testy a Clippy a nahrají artefakty dostupné v detailu běhu po dobu 30 dnů.

Každý artefakt obsahuje `.pkg` instalátor do `/Applications`, `.dmg` s aplikací a odkazem na Applications a `.sha256` soubor. Instalátor ani aplikace se při ověřování nespouštějí. Lokálně:

```sh
./scripts/package-macos.sh
# Výstup: dist/installers/
```

Pro GitHub Release uprav `Cargo.toml` i `resources/Info.plist` na novou verzi, aktualizuj `Cargo.lock`, commitni a pushni tag `v<verze>`. Tag musí odpovídat `CFBundleShortVersionString`. Až projdou oba buildy, workflow publikuje v tomto repozitáři Release s oběma architekturami. Soukromé repo znamená i soukromé artefakty a vydání.

**Podpisy:** CI zatím používá ad hoc podpis aplikace; `.pkg` není podepsaný certifikátem Developer ID Installer a balíčky nejsou notarizované Applem. Gatekeeper je proto nemusí otevřít běžným dvojklikem. Pro distribuci bez tohoto omezení je nutné doplnit Developer ID Application/Installer a notarizaci. Pipeline neexportuje lokální klíče ani certifikáty. Tyto instalační artefakty nezveřejňují ani nepřepisují podepsané aktualizační feedy Rustlet/Sparkle; stávající ověřování aktualizací zůstává aktivní.
