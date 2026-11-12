# Формат пользовательских исторических данных

Поставщика и период выбирает пользователь. Загрузчик данных не поставляется.
Команда `import-history <db> <root> [symbol]` импортирует подготовленные минуты;
без symbol используется EURUSD. Рабочая база и её происхождение не переписываются.

В quality-report.json поле source — непустое название фактического источника.
Оно сохраняется в SQLite и отчёте бэктеста, не заменяется названием формата.
Для одного инструмента бэктест требует единый источник во всех каталогах.
Старые базы и отчёты с заполненным source продолжают читаться.

Это не универсальный CSV-импорт: текущий адаптер требует timezone_conversion
со значением `fixed EST UTC-05:00 to UTC; no DST`. Архивные тики используют
фиксированный UTC−5, без DST; подготовленные минуты — UTC. Другие часовые
пояса нужно явно преобразовать до запуска; нельзя просто переименовать источник.

Корневой каталог содержит manifest.json, quality-report.json, raw/ и prepared/.
manifest.json содержит files — список объектов month (YYYY-MM), file (относительный
путь ZIP) и sha256. ZIP содержит один CSV с именем DAT_ASCII_<SYMBOL>_T_*.csv.
Строки тиков: YYYYMMDD HHMMSSmmm,Bid,Ask,Volume; объём не используется.

quality-report.json содержит symbol, source, timezone_conversion,
timestamp_reversals и outputs; для импорта также обязателен missing_months.
outputs сопоставляет имя подготовленного файла с bytes и sha256.
Минутные файлы импорта называются <SYMBOL>-M1-bid-ask-<период>.csv.
Поля CSV: timestamp_utc, timestamp_ms, bar_end_utc, bid_open/high/low/close,
ask_open/high/low/close, quote_count, gap_before_minutes (каждый OHLC — отдельная колонка).
Цена — десятичная строка, внутренний масштаб 1 000 000. Пустые минуты не заполняются.

Каждый элемент timestamp_reversals задаёт excluded_from_local_minute и
excluded_through_local_minute в формате YYYYMMDD HHMM; обе граничные минуты
исключаются. Исходные котировки сохраняются, неоднозначные интервалы не сортируются
в вымышленные свечи. Временная сетка стратегии не меняет часовой пояс архива.

Примеры путей в backtest/data-sources.json и Compose нейтральны: разместите
свои подготовленные наборы в data/history/<SYMBOL> либо измените оба файла.

## Выгрузка баров MetaTrader 5

Команда `prepare-mt5 <export.csv> <root> <symbol> <ask_spread> <source>`
преобразует выгрузку M1 из MT5 («Символы → Бары → Экспорт баров») в тот же
каталог: prepared/<SYMBOL>-M1-bid-ask-YYYY-MM.csv, prepared/gaps.csv и
quality-report.json. Затем используется обычный import-history.
Каталог root должен не существовать или быть пустым; файлы не перезаписываются.

Вход: заголовок `<DATE> <TIME> <OPEN> <HIGH> <LOW> <CLOSE> <TICKVOL> <VOL> <SPREAD>`
через табуляцию, время YYYY.MM.DD HH:MM:SS, цены bid, строки строго по возрастанию.
Время сервера считается равным America/New_York + 7 часов (UTC+2 зимой,
UTC+3 летом, граница суток — 17:00 New York). Несуществующее или неоднозначное
локальное время отклоняется. timezone_conversion отчёта:
`MT5 server time = America/New_York +7h to UTC`.

Bid OHLC берётся из выгрузки. Колонка `<SPREAD>` не используется: ask строится
как bid + ask_spread во всех четырёх ценах, ask_spread — целое число миллионных
долей цены (100 = 1 пункт EURUSD). Модель ask записывается в отчёт полем
ask_model. quote_count — `<TICKVOL>`; минута без тиков отклоняется.
gap_before_minutes — число отсутствующих минут перед строкой (для первой 0).
Пропущенные минуты не заполняются. gaps.csv перечисляет разрывы длиннее
60 минут вне выходных: gap_start_utc, gap_end_utc, missing_minutes.
missing_months — календарные месяцы UTC между первой и последней минутой
без единой строки.
