# shellwing in de. Each message is found by its English, the
# comment above it; scripts/i18n.py keeps this file in step with the code.

# An edit of {remote} was not written back to the server; the file is kept as {path}.
t62769e5b = Eine Bearbeitung von { $remote } wurde nicht auf den Server zurückgeschrieben; die Datei liegt unter { $path }.

# {n} edited file was not written back to the server; it is kept in Downloads: {files}. | {n} edited files were not written back to the server; they are kept in Downloads: {files}.
td8a345bd =
    { $n ->
        [one] { $n } bearbeitete Datei wurde nicht auf den Server zurückgeschrieben; sie liegt in Downloads: { $files }.
       *[other] { $n } bearbeitete Dateien wurden nicht auf den Server zurückgeschrieben; sie liegen in Downloads: { $files }.
    }

# no editor command
tbf2a5d3a = kein Editor-Befehl

# no text editor found
t6256b3da = kein Texteditor gefunden

# The command is {size} KB; {max} KB at most.
ta78b9fbd = Der Befehl hat { $size } KB; höchstens { $max } KB.

# {name}: a name takes {max} characters at most.
t190cce46 = { $name }: Ein Name hat höchstens { $max } Zeichen.

# {name} is {size} KB; a value takes {max} KB at most.
t8a0064ca = { $name } hat { $size } KB; ein Wert hat höchstens { $max } KB.

# {count} variables; {max} at most.
t1c9f770c = { $count } Variablen; höchstens { $max }.

# The variables come to {size} KB; {max} KB at most.
tdbf76954 = Die Variablen ergeben { $size } KB; höchstens { $max } KB.

# The file is {size} KB; {max} KB at most.
tf2bc7ef2 = Die Datei hat { $size } KB; höchstens { $max } KB.

# The note is {size} KB; {max} KB at most.
td5e1ea7c = Die Notiz hat { $size } KB; höchstens { $max } KB.

# Name
t0fe07306 = Name

# The connection ended.
tc85117e2 = Die Verbindung wurde beendet.

# The connection ended while writing back.
t424caf2a = Die Verbindung wurde beim Zurückschreiben beendet.

# Not connected.
tc9ec8929 = Nicht verbunden.

# Cannot make a folder to edit in: {err}
t4d82727e = Ordner zum Bearbeiten kann nicht angelegt werden: { $err }

# the text editor
t91cdab0c = den Texteditor

# Cannot start {who}: {e}
tc39185d1 = Kann { $who } nicht starten: { $e }

# Not connected: written back once it is.
t8345869d = Nicht verbunden: wird zurückgeschrieben, sobald die Verbindung steht.

# Not written back: {e}
t6b172035 = Nicht zurückgeschrieben: { $e }

# Cannot move it to Downloads: {e}
t33407575 = Kann nicht nach Downloads verschoben werden: { $e }

# not written back to the server: discard your changes?
t1adcdad0 = nicht auf den Server zurückgeschrieben: Änderungen verwerfen?

# opening…
t185c4e41 = wird geöffnet…

# in the editor; each save is written back
tcf8122af = im Editor; jedes Speichern wird zurückgeschrieben

# written back at {t}
t4a0a44e2 = zurückgeschrieben um { $t }

# writing back…
ta2088cca = wird zurückgeschrieben…

# changed on the server since you opened it
tf8056821 = seit dem Öffnen auf dem Server geändert

# not on the server; moved to Downloads as {name}
tbc889b90 = nicht auf dem Server; als { $name } nach Downloads verschoben

# Open
t538b10e9 = Öffnen

# Stop
t4b7f7705 = Stoppen

# Discard
t1e706c21 = Verwerfen

# Keep in Downloads
t3052af2e = In Downloads behalten

# Cancel
t35afca3b = Abbrechen

# Show
t29742e5c = Anzeigen

# Overwrite
t78ff0e92 = Überschreiben

# Retry
t8036af59 = Erneut versuchen

# Upload
t826f7f7c = Hochladen

# Not deleted: {err}
t74196840 = Nicht gelöscht: { $err }

# Edit in text editor
td8d39fb8 = Im Texteditor bearbeiten

# Edit in {editor}
te44cd57b = In { $editor } bearbeiten

# Open with default app
t5ef80ec5 = Mit Standard-App öffnen

# Download
t6964bb19 = Herunterladen

# Download As…
t955e4fd1 = Herunterladen als…

# New folder…
t4f4860f1 = Neuer Ordner…

# Upload…
tb19d9b38 = Hochladen…

# Refresh
t0815aad4 = Aktualisieren

# Copy path
tf165cb2b = Pfad kopieren

# Rename…
t8628af0d = Umbenennen…

# Delete…
t2fed07aa = Löschen…

# link
t0ddb0669 = Link

# {percent}% · {rate}/s
td8a3a0d4 = { $percent }% · { $rate }/s

# starting…
t1a0ebe8f = startet…

# already there
te2f6c72d = bereits vorhanden

# Merge
t8b725bc7 = Zusammenführen

# Replace
te4d54f63 = Ersetzen

# Skip
tc197c7e2 = Überspringen

# done · {size}
t38ca2d3b = fertig · { $size }

# Files
te71d1b30 = Dateien

# Home
t52f50eae = Home

# New folder
t1926d127 = Neuer Ordner

# Rename {old}
t2a3708da = { $old } umbenennen

# Enter to save · Esc to cancel
t9abaa44e = Enter zum Speichern · Esc zum Abbrechen

# Delete the folder {name} and everything in it?
t219d4be7 = Ordner { $name } mit allem Inhalt löschen?

# Delete {name}?
t4397aaa4 = { $name } löschen?

# Delete
t5797ea6a = Löschen

# Opening SFTP…
t6c4d75a4 = SFTP wird geöffnet…

# Loading…
tc4e2f181 = Lädt…

# Try again
t213e90fa = Erneut versuchen

# Empty folder.
t56515154 = Leerer Ordner.

# Right-click a file or folder for its actions; drop files here to upload them.
t1377d961 = Rechtsklick auf eine Datei oder einen Ordner zeigt die Aktionen; zieh Dateien hierher, um sie hochzuladen.

# Production
t89cb4f3a = Produktion

# A name is needed.
tad1c356c = Ein Name ist erforderlich.

# Another group has this name.
tc0f2f206 = Eine andere Gruppe hat diesen Namen.

# There is no config directory to save in.
t24dff34a = Es gibt kein Konfigurationsverzeichnis zum Speichern.

# This group cannot be found in hosts.toml.
tbc9872ca = Diese Gruppe ist nicht in hosts.toml zu finden.

# New group
tb7b3f75a = Neue Gruppe

# Group in hosts.toml
tfc009326 = Gruppe in hosts.toml

# Not saved yet
t8662a2eb = Noch nicht gespeichert

# Same as the app
tf3e994b9 = Wie die App

# Theme for its hosts
t9d045818 = Design für ihre Hosts

# No hosts yet. Choose this group in a host's editor.
t73e08796 = Noch keine Hosts. Wähle diese Gruppe im Editor eines Hosts.

# Choose the group in a host's editor to put the host in it.
t880fbefc = Wähle die Gruppe im Editor eines Hosts, um den Host hinzuzufügen.

# {n} host: {names} | {n} hosts: {names}
t79084a5c =
    { $n ->
        [one] { $n } Host: { $names }
       *[other] { $n } Hosts: { $names }
    }

# Group
t0574902c = Gruppe

# Color
te5b43cf8 = Farbe

# A host's own theme wins over its group's.
tad991bdd = Das eigene Design eines Hosts hat Vorrang vor dem seiner Gruppe.

# Delete {title}?
ta14ea5b3 = { $title } löschen?

# Delete {title}? Its {n} host stay. | Delete {title}? Its {n} hosts stay.
t7eb2e426 =
    { $n ->
        [one] { $title } löschen? Ihr { $n } Host bleibt erhalten.
       *[other] { $title } löschen? Ihre { $n } Hosts bleiben erhalten.
    }

# Save
t4d2d5d68 = Speichern

# Automatic
t616a6784 = Automatisch

# Key file
t6ed7c612 = Schlüsseldatei

# Password
t2cc30838 = Passwort

# Agent only
t80e1a754 = Nur Agent

# optional
t49c5d8f9 = optional

# 10.0.0.5 or host.example.com
t685963cf = 10.0.0.5 oder host.example.com

# your login name
t62d0236a = dein Anmeldename

# a host here, an ssh alias, or user@host
tdcf5544b = ein Host hier, ein ssh-Alias oder user@host

# Staging
tcb81d7c6 = Staging

# Another host has this name.
te27e50cd = Ein anderer Host hat diesen Namen.

# No spaces.
t02933ecd = Keine Leerzeichen.

# Another host uses this.
t946bf8e8 = Ein anderer Host nutzt dies.

# An address is needed.
t140a0aeb = Eine Adresse ist erforderlich.

# 1 to 65535.
t1bbc0d91 = 1 bis 65535.

# Choose a key.
t6e8ece2a = Wähle einen Schlüssel.

# Seconds, 0 to 3600 (0: none).
tc0567af7 = Sekunden, 0 bis 3600 (0: keiner).

# A host cannot jump through itself.
t272071b9 = Ein Host kann nicht über sich selbst springen.

# new key
t2aaac45a = neuer Schlüssel

# the vault is not open
t97a4b927 = der Tresor ist nicht geöffnet

# No key made: {e}
t4e14e42b = Kein Schlüssel erstellt: { $e }

# This host cannot be found in hosts.toml.
ta0e18527 = Dieser Host ist nicht in hosts.toml zu finden.

# {name} copy
t0d616a81 = { $name } Kopie

# Choose
t73c66160 = Auswählen

# host name
t8465975e = Hostname

# Reaches
t9a71395c = Erreicht

# Key
tcd1ac90c = Schlüssel

# Through
t2e38a41a = Über

# Forwards
t5567756b = Weiterleitungen

# It could not be read.
t54886f1b = Konnte nicht gelesen werden.

# From ~/.ssh/config
t85f5968b = Aus ~/.ssh/config

# Host from ~/.ssh/config
t6a7dc843 = Host aus ~/.ssh/config

# shellwing reads this file but never writes to it. Edit it in your editor, or make a copy here to add a group, a theme, forwards or a saved password.
t509e342f = shellwing liest diese Datei, schreibt aber nie hinein. Bearbeite sie in deinem Editor oder erstelle hier eine Kopie, um eine Gruppe, ein Design, Weiterleitungen oder ein gespeichertes Passwort hinzuzufügen.

# Open config file
t54ea8995 = Konfigurationsdatei öffnen

# Copy to my hosts
t72c32fa6 = Zu meinen Hosts kopieren

# New host
t5c18d08d = Neuer Host

# Saved in hosts.toml
teae7befe = In hosts.toml gespeichert

# Connection
t09ea4ff9 = Verbindung

# Alias for the command line
ted5013c8 = Alias für die Befehlszeile

# Address
t700560f3 = Adresse

# Port
te2c53cc6 = Port

# User
te0a63e12 = Benutzer

# Sign in
t744be623 = Anmeldung

# {name} · in the vault · {kind}
tbf1c044e = { $name } · im Tresor · { $kind }

# Choose a key
t32d1fb40 = Schlüssel wählen

# {name}   in the vault · {kind}
ta6d50f9b = { $name }   im Tresor · { $kind }

# Another key file…
t6f4497c1 = Andere Schlüsseldatei…

# + Generate a key for this host
ta1a39e38 = + Schlüssel für diesen Host erzeugen

# + Generate a key for {host}
t2065d130 = + Schlüssel für { $host } erzeugen

# the host
t39dac2c6 = dem Host

# {name} is new: {host} does not have it yet. Saving and installing signs in once another way (a password, say), then adds it.
t7ecf0d6d = { $name } ist neu: Auf { $host } fehlt er noch. Speichern und Installieren meldet sich einmal auf anderem Weg an (etwa per Passwort) und fügt ihn dann hinzu.

# Save and install on {host}
t1707b1d3 = Speichern und auf { $host } installieren

# Tries the agent, then your keys in ~/.ssh and the vault, then asks for a password, like ssh.
t786a9bff = Versucht den Agent, dann deine Schlüssel in ~/.ssh und im Tresor, dann fragt es nach einem Passwort, wie ssh.

# Kept in the vault: no passphrase to ask for.
t74ee0f69 = Im Tresor gespeichert: keine Passphrase nötig.

# Passphrase: asked for when needed, and can be saved in the vault.
tbdfc0bf6 = Passphrase: wird bei Bedarf abgefragt und kann im Tresor gespeichert werden.

# Asked for when connecting, and can be saved in the vault.
ta6140863 = Wird beim Verbinden abgefragt und kann im Tresor gespeichert werden.

# Uses the keys in your ssh-agent.
t3a4ba988 = Nutzt die Schlüssel in deinem ssh-agent.

# + Add jump
t3f09cb93 = + Jump-Host hinzufügen

# Route
td7d630be = Route

# Jump through, in order
tfc8a9b1f = Über diese Hosts, der Reihe nach

# Enter adds what you typed · Esc stops adding
ta13c2c20 = Enter fügt das Eingegebene hinzu · Esc beendet das Hinzufügen

# + Add forward
tcfdf6ef4 = + Weiterleitung hinzufügen

# Port forwarding
tb033f949 = Portweiterleitung

# Set up every time this host connects.
t0d74aaf9 = Wird bei jeder Verbindung dieses Hosts eingerichtet.

# Sync
t89a71bba = Sync

# Keep on this device only
t20b0191f = Nur auf diesem Gerät behalten

# Not synced: other devices do not see this host.
t657130a6 = Nicht synchronisiert: andere Geräte sehen diesen Host nicht.

# Synced with your other devices when sync is on.
tb80f1c8f = Wird mit deinen anderen Geräten synchronisiert, wenn Sync aktiv ist.

# None
t304ff7fb = Keine

# {n} (this host only)
tb6562d26 = { $n } (nur dieser Host)

# Organize
t6ddb17ec = Ordnen

# Tags
t76bda0c0 = Tags

# + New group…
t9d11ac85 = + Neue Gruppe…

# New group's name
t2e4cb82f = Name der neuen Gruppe

# Made when you save this host.
taf695c2c = Wird beim Speichern dieses Hosts erstellt.

# Shown on the tab and the pane border
t090fc8f1 = Wird im Tab und am Rand des Bereichs angezeigt

# Favorite: listed first in the host list
t527b0c5e = Favorit: steht in der Hostliste oben

# Terminal
t93c562d1 = Terminal

# Theme
t4eaf7c92 = Design

# Run after connecting
t2b3c50ce = Nach dem Verbinden ausführen

# Typed into the shell once it is ready, after every connect and reconnect.
t2ac048fc = Wird in die Shell eingegeben, sobald sie bereit ist, nach jedem Verbinden und Wiederverbinden.

# App setting
t4e528c40 = App-Einstellung

# Ask
t7cfd61b2 = Fragen

# Accept new
ta6e1de9b = Neue akzeptieren

# Strict
t6d561894 = Strikt

# Always
tad4bbd04 = Immer

# Never
tb2ff2b29 = Nie

# none
tada7afdb = keine

# {s} s
te78896af = { $s } s

# Advanced
t60f3de7b = Erweitert

# Unknown host key
t57114866 = Unbekannter Host-Schlüssel

# The app's setting is {app_key} now. A changed key is always refused.
t6669805b = Die App-Einstellung ist derzeit { $app_key }. Ein geänderter Schlüssel wird immer abgelehnt.

# Session log
t54c8d181 = Sitzungsprotokoll

# Always: each session is logged to a file from its start. Never: not even when the app logs every session. The app logs {logs} now.
t9ccf92c6 = Immer: jede Sitzung wird von Beginn an in eine Datei protokolliert. Nie: auch dann nicht, wenn die App jede Sitzung protokolliert. Die App protokolliert derzeit { $logs }.

# every session
t4dbfb628 = jede Sitzung

# Keepalive
t3d5b35a9 = Keepalive

# Seconds between checks; 0: none. Empty: the app's setting ({app_alive}).
t66f5835d = Sekunden zwischen Prüfungen; 0: keine. Leer: Einstellung der App ({ $app_alive }).

# Forward my agent to this host
t80664148 = Meinen Agent an diesen Host weiterleiten

# While you are connected, whoever is root on this host can sign in elsewhere with your keys too. Only for hosts you trust.
t741afb89 = Solange du verbunden bist, kann jeder mit Root-Rechten auf diesem Host sich auch anderswo mit deinen Schlüsseln anmelden. Nur für Hosts, denen du vertraust.

# From this host you could then git push or ssh further with the keys on this computer; they are never copied there.
t01f5f399 = Von diesem Host aus kannst du dann mit den Schlüsseln dieses Computers git push ausführen oder dich per ssh weiterverbinden; sie werden nie dorthin kopiert.

# Some fields need a look.
teaa35f1a = Einige Felder sind zu prüfen.

# Delete {title} from hosts.toml?
t596a83da = { $title } aus hosts.toml löschen?

# Duplicate
tfc330336 = Duplizieren

# Save and connect
t83731197 = Speichern und verbinden

# Ed25519 (recommended)
t7294b934 = Ed25519 (empfohlen)

# A name, to tell your keys apart
t6c2bbece = Ein Name, um deine Schlüssel zu unterscheiden

# Copied the public key.
t66ef7116 = Öffentlicher Schlüssel kopiert.

# Not renamed: {e}
t836adf69 = Nicht umbenannt: { $e }

# Deleted {name}.
t0c7abd07 = { $name } gelöscht.

# Not deleted: {e}
t12e9b880 = Nicht gelöscht: { $e }

# The two passphrases differ.
teb0402b1 = Die beiden Passphrasen stimmen nicht überein.

# the key is not in the vault
t7ab0b044 = der Schlüssel ist nicht im Tresor

# the key does not read
t25447ed3 = der Schlüssel ist nicht lesbar

# Written to {path} and its .pub.
td30d7d74 = In { $path } und die zugehörige .pub geschrieben.

# Not written: {e}
t3ccd278a = Nicht geschrieben: { $e }

# Cannot read {path}: {e}
t7b24cd24 = { $path } nicht lesbar: { $e }

# Its passphrase
t279e8bbb = Zugehörige Passphrase

# That passphrase does not open it.
t05c6a9c1 = Diese Passphrase öffnet ihn nicht.

# {path} is not an OpenSSH private key.
te16bf944 = { $path } ist kein privater OpenSSH-Schlüssel.

# Saved the passphrase of {file_name}: it is not asked for again.
t181d393f = Passphrase von { $file_name } gespeichert: sie wird nicht mehr abgefragt.

# Not saved: {e}
ta9354472 = Nicht gespeichert: { $e }

# {name} is in the vault now, and syncs to your other devices.
td3d8b162 = { $name } ist jetzt im Tresor und wird mit deinen anderen Geräten synchronisiert.

# Not copied: {e}
t03370a6b = Nicht kopiert: { $e }

# {name} is in the ssh-agent now, until it stops.
tf6d081d4 = { $name } ist jetzt im ssh-agent, bis dieser beendet wird.

# Not added: {e}
t6ff07db9 = Nicht hinzugefügt: { $e }

# {name} is out of the ssh-agent.
t5ed55b5e = { $name } ist nicht mehr im ssh-agent.

# Not removed: {e}
t7b4feb23 = Nicht entfernt: { $e }

# The key is not in the vault.
t16ca2ffe = Der Schlüssel ist nicht im Tresor.

# Forgot the passphrase of {path}: it is asked for again.
t4b242af5 = Passphrase von { $path } vergessen: sie wird wieder abgefragt.

# Not forgotten: {e}
t0e3fb3a1 = Nicht vergessen: { $e }

# Import
t573e0af4 = Importieren

# not used by a host yet
t7a67a5ad = noch von keinem Host genutzt

# used by {one}
tefe52e8f = genutzt von { $one }

# used by {a}, {b}
t72e17f0a = genutzt von { $a }, { $b }

# used by {n} host | used by {n} hosts
tb629858a =
    { $n ->
        [one] genutzt von { $n } Host
       *[other] genutzt von { $n } Hosts
    }

# Copy public key
tccf653ee = Öffentlichen Schlüssel kopieren

# Install on…
t0dfc84df = Installieren auf…

# no passphrase
ta9025922 = keine Passphrase

# passphrase saved in the vault
t2c083e8e = Passphrase im Tresor gespeichert

# passphrase
t643fc8a5 = Passphrase

# also in the vault
t867e5d66 = auch im Tresor

# the same key as {path}
t0e1c7d80 = derselbe Schlüssel wie { $path }

# the same key as {name} in the vault
tdfa4ba60 = derselbe Schlüssel wie { $name } im Tresor

# only in the agent
t3c2bc198 = nur im Agent

# the agent's key
t7e7cc13c = Schlüssel des Agents

# (no comment)
t786863d8 = (kein Kommentar)

# Export to file…
t7b929ace = In Datei exportieren…

# A passphrase, or none
t79f95085 = Eine Passphrase oder keine

# The passphrase again
tc3b3d2cc = Passphrase wiederholen

# Remove from ssh-agent
td745f8e9 = Aus ssh-agent entfernen

# Add to ssh-agent
t44341cdb = Zu ssh-agent hinzufügen

# Copy to vault
t7ac5f049 = In den Tresor kopieren

# Save its passphrase…
tf91e6d9a = Passphrase speichern…

# Forget its saved passphrase
tc70cfa3b = Gespeicherte Passphrase vergessen

# Generate a key
t1824e4f6 = Schlüssel erzeugen

# Only for you, to tell your keys apart.
te5c40fac = Nur für dich, um deine Schlüssel zu unterscheiden.

# Type
td155d06d = Typ

# Ed25519 is short, fast and taken by OpenSSH 6.5 and later. RSA for very old servers.
t4146388b = Ed25519 ist kurz, schnell und wird ab OpenSSH 6.5 akzeptiert. RSA für sehr alte Server.

# Comment
t984f9ffe = Kommentar

# Written at the end of the public key: what a server's authorized_keys shows.
t2a1f71fd = Steht am Ende des öffentlichen Schlüssels: das, was authorized_keys auf einem Server zeigt.

# Kept in the vault, encrypted, and synced to your other devices, so it needs no passphrase. To use it with ssh or git too, export it to a file, with a passphrase if you like.
t504dc243 = Wird verschlüsselt im Tresor gespeichert und mit deinen anderen Geräten synchronisiert, daher braucht er keine Passphrase. Um ihn auch mit ssh oder git zu nutzen, exportiere ihn in eine Datei, auf Wunsch mit Passphrase.

# Generating…
t12235a6b = Wird erzeugt…

# Generate
tc75562b2 = Erzeugen

# {name} is ready
t17060a6b = { $name } ist bereit

# A server lets this key in once its public key is in the server's ~/.ssh/authorized_keys. Install it on a host from here, or copy it into a web console (GitHub, GitLab, a cloud provider).
tb989d6aa = Ein Server lässt diesen Schlüssel zu, sobald sein öffentlicher Schlüssel in ~/.ssh/authorized_keys auf dem Server steht. Installiere ihn von hier aus auf einem Host oder kopiere ihn in eine Webkonsole (GitHub, GitLab, ein Cloud-Anbieter).

# Public key
t9981f45f = Öffentlicher Schlüssel

# Copy
t658f3664 = Kopieren

# Done
t8dd31791 = Fertig

# Install on a host…
ta8313af0 = Auf einem Host installieren…

# Rename the key
t2c552c07 = Schlüssel umbenennen

# Rename
tc80631f3 = Umbenennen

# Servers that have its public key will not let it in from here any more, from any of your devices.
tf9cc53bc = Server mit seinem öffentlichen Schlüssel lassen ihn von hier aus nicht mehr zu, von keinem deiner Geräte.

# {hosts} sign in with it: they will try the other ways (the agent, ~/.ssh, a password) instead. Servers that have its public key keep it until you remove it there.
t20cb489a = { $hosts } melden sich damit an: sie versuchen stattdessen die anderen Wege (Agent, ~/.ssh, Passwort). Server mit seinem öffentlichen Schlüssel behalten ihn, bis du ihn dort entfernst.

# Export {name}
t78124df8 = { $name } exportieren

# Written as an OpenSSH key file, for ssh, git and other tools, with its .pub beside it. A passphrase locks the file; the key in the vault stays as it is.
teb4d5edb = Wird als OpenSSH-Schlüsseldatei für ssh, git und andere Tools geschrieben, mit der .pub daneben. Eine Passphrase sperrt die Datei; der Schlüssel im Tresor bleibt unverändert.

# Passphrase
t191a2dc5 = Passphrase

# Again
t58fb56ef = Wiederholen

# Choose where…
t65990425 = Speicherort wählen…

# Unlock {path}
ta8940d72 = { $path } entsperren

# This key file has a passphrase. Type it to copy the key into the vault, where it needs none; the file stays as it is.
tc1aba63d = Diese Schlüsseldatei hat eine Passphrase. Gib sie ein, um den Schlüssel in den Tresor zu kopieren, wo er keine braucht; die Datei bleibt unverändert.

# This key file has a passphrase. Type it to hand the key to the ssh-agent; the file stays as it is.
td51093f3 = Diese Schlüsseldatei hat eine Passphrase. Gib sie ein, um den Schlüssel an den ssh-agent zu übergeben; die Datei bleibt unverändert.

# Save the passphrase of {path}
ta6e38b23 = Passphrase von { $path } speichern

# Kept in the vault, encrypted, and synced: shellwing then signs in with this key without asking, here and on your other devices.
tf45d84f1 = Wird verschlüsselt im Tresor gespeichert und synchronisiert: shellwing meldet sich dann ohne Nachfrage mit diesem Schlüssel an, hier und auf deinen anderen Geräten.

# Asking the agent…
tabb2eee4 = Agent wird abgefragt…

# No agent, or it holds no keys.
tcad7e01e = Kein Agent, oder er hat keine Schlüssel.

# Keys
t602b32ed = Schlüssel

# Import key file…
t68acce01 = Schlüsseldatei importieren…

# Generate key…
t0d000853 = Schlüssel erzeugen…

# Keys for signing in to servers. Keys you generate or import here are kept in the vault, encrypted, and sync to your other devices.
t9da1da72 = Schlüssel zum Anmelden an Servern. Hier erzeugte oder importierte Schlüssel werden verschlüsselt im Tresor gespeichert und mit deinen anderen Geräten synchronisiert.

# The vault cannot be read: {p}
t650c9448 = Der Tresor ist nicht lesbar: { $p }

# In the vault
ta13ebcfd = Im Tresor

# synced · used only by shellwing
t0714e99d = synchronisiert · nur von shellwing genutzt

# None yet: generate one, or copy one of your key files here.
t95a0c0ba = Noch keine: erzeuge einen oder kopiere eine deiner Schlüsseldateien hierher.

# In ~/.ssh
t6420c141 = In ~/.ssh

# key files on this computer, shared with ssh and git
t77dbc96f = Schlüsseldateien auf diesem Computer, gemeinsam mit ssh und git genutzt

# No key files.
t4c7991a8 = Keine Schlüsseldateien.

# In ssh-agent
t98288f7c = Im ssh-agent

# offered first when connecting
tc83ab408 = wird beim Verbinden zuerst angeboten

# Hosts set to Automatic try the agent, then ~/.ssh, then the vault, then ask for a password.
t0d28103e = Hosts mit Automatisch versuchen den Agent, dann ~/.ssh, dann den Tresor und fragen dann nach einem Passwort.

# Find a host, or type one to find hashed lines
te7877ac6 = Host suchen oder eingeben, um gehashte Zeilen zu finden

# Cannot read known_hosts: {e}
t57b65f0a = known_hosts nicht lesbar: { $e }

# Removed line {line}: the next connection to it asks about its key. The file as it was is known_hosts.old.
tcea0bcc6 = Zeile { $line } entfernt: die nächste Verbindung dorthin fragt nach seinem Schlüssel. Die Datei im alten Zustand liegt in known_hosts.old.

# No hosts yet: each one you trust is added here.
t72b46d71 = Noch keine Hosts: jeder, dem du vertraust, wird hier hinzugefügt.

# None matches.
tc9b5b29c = Kein Treffer.

# a hashed name
t87a3980c = ein gehashter Name

# {typed} (hashed)
t63339ce3 = { $typed } (gehasht)

# {names} and {hashed} hashed
ta6fa2480 = { $names } und { $hashed } gehashte

# revoked
tb4e706f5 = widerrufen

# certificate authority
t5299ff65 = Zertifizierungsstelle

# {fingerprint} · line {line}
t25339948 = { $fingerprint } · Zeile { $line }

# Remove
t21a5901d = Entfernen

# Known hosts
tc951355f = Bekannte Hosts

# Open known_hosts
t61ed7186 = known_hosts öffnen

# The servers you trust, and their keys, from {path}: the same file ssh reads. A server showing another key is refused, and its pane says what changed. Removing one makes the next connection ask about it again.
tfb280692 = Die Server, denen du vertraust, und ihre Schlüssel, aus { $path }: dieselbe Datei, die ssh liest. Ein Server mit einem anderen Schlüssel wird abgelehnt, und sein Bereich zeigt, was sich geändert hat. Nach dem Entfernen fragt die nächste Verbindung erneut nach.

# {n} key | {n} keys
tc219ff12 =
    { $n ->
        [one] { $n } Schlüssel
       *[other] { $n } Schlüssel
    }

# No config directory to save to.
ta5f37ee4 = Kein Konfigurationsverzeichnis zum Speichern.

# Theme `{want}` not found; using the default.
t1cda442a = Design `{ $want }` nicht gefunden; Standard wird verwendet.

# {host} sets its own theme in hosts.toml; change it there.
tec1b7d01 = { $host } legt in hosts.toml ein eigenes Design fest; ändere es dort.

# Theme `{id}` not found.
tbae3cb18 = Design `{ $id }` nicht gefunden.

# Blue and slate
t35054fdd = Blau und Schiefer

# Purple and pink
t6bae17ce = Lila und Pink

# Green and teal
tfc91f0a1 = Grün und Petrol

# Warm
t04a11b58 = Warm

# Neutral
te9330778 = Neutral

# Gradient
t993c73c5 = Verlauf

# Gradient, light
td685ec1b = Verlauf, hell

# Light, cool
tf5e42e16 = Hell, kühl

# Light, warm
tdd571ace = Hell, warm

# Your themes
tb2e83d5c = Deine Designs

# New note
t788c7123 = Neue Notiz

# Title
t24d471a9 = Titel

# No category
t44662d06 = Keine Kategorie

# Write here; Markdown shows when previewed.
t05ddff4f = Hier schreiben; Markdown erscheint in der Vorschau.

# It is not in the vault any more.
td11af590 = Sie ist nicht mehr im Tresor.

# this note
t4545c373 = diese Notiz

# It goes from the vault, and from your other devices once they sync.
t3c6666da = Es wird aus dem Tresor entfernt und von deinen anderen Geräten, sobald sie synchronisieren.

# Edit it in another app?
t1719e0f2 = In einer anderen App bearbeiten?

# While it is open there, a copy in plain text is on this computer's disk (in a folder only you can read), and the editor may keep copies of its own, such as backups or history. The copy goes when you are done.
tbcec8b45 = Solange sie dort offen ist, liegt eine Kopie als reiner Text auf der Festplatte dieses Computers (in einem Ordner, den nur du lesen kannst), und der Editor kann eigene Kopien behalten, etwa Sicherungen oder Verlauf. Die Kopie verschwindet, wenn du fertig bist.

# Cannot make the copy: {e}
td59a6621 = Die Kopie lässt sich nicht anlegen: { $e }

# It is not text any more.
te3591497 = Es ist kein Text mehr.

# Not read back: {e}
tf04d4bce = Nicht zurückgelesen: { $e }

# Insert into {pane}
t3d5eaffc = In { $pane } einfügen

# No shell to insert into
t825e7370 = Keine Shell zum Einfügen

# The vault's passphrase
tb66334ea = Passphrase des Tresors

# Wrong passphrase.
t2ebec270 = Falsche Passphrase.

# For a host ▾
tfa622581 = Für einen Host ▾

# No hosts yet.
t71734b03 = Noch keine Hosts.

# Saving…
tbd79b37d = Wird gespeichert…

# Click the text to edit it
t63928671 = Zum Bearbeiten auf den Text klicken

# Saved
t8a6979e4 = Gespeichert

# Plain text
tb7fc0ade = Reiner Text

# Edit
tc2c76cb1 = Bearbeiten

# Preview
tfdda4cf7 = Vorschau

# Sensitive
tf603feef = Vertraulich

# Edit in another app…
t1c6d0a34 = In einer anderen App bearbeiten…

# The vault is locked: unlock it in the main window.
te327c98e = Der Tresor ist gesperrt: Entsperre ihn im Hauptfenster.

# Sensitive: its text shows once it is you.
tb81fab1b = Vertraulich: Der Text erscheint, sobald bestätigt ist, dass du es bist.

# Show the text
t0ff690fe = Text anzeigen

# Confirm it is you in the system's prompt…
t3ab56316 = Bestätige im Dialog des Systems, dass du es bist…

# No passphrase guards the vault, so anyone at this device can show it.
t41e36c52 = Keine Passphrase schützt den Tresor, also kann jeder an diesem Gerät ihn anzeigen.

# Open in another app: what it saves comes here, and is kept.
t340e0631 = In einer anderen App geöffnet: Was sie speichert, kommt hierher und bleibt.

# Nothing written yet: Edit to write.
t21ced1e5 = Noch nichts geschrieben: Zum Schreiben auf Bearbeiten.

# Untitled note
t4ce277c4 = Notiz ohne Titel

# Hosts
tcf9aefb4 = Hosts

# Recent
t50e888fe = Zuletzt

# Snippets
t648fcbf7 = Snippets

# Environments
t273b1ccb = Umgebungen

# Config files
t16261310 = Konfigurationsdateien

# Notes
t0ff01b6a = Notizen

# Actions
te65fb504 = Aktionen

# Settings
t4b058728 = Einstellungen

# Find a host, or type user@host
t18f005c7 = Host suchen oder user@host eingeben

# Find a saved password or passphrase
t8452d3bc = Gespeichertes Passwort oder Passphrase suchen

# Find a theme
ta5e61028 = Design suchen

# Connect
tdb89a899 = Verbinden

# Connect to {typed}
t51c5c888 = Mit { $typed } verbinden

# a new address; a Host block in ~/.ssh/config applies
t69b58e6c = eine neue Adresse; ein Host-Block in ~/.ssh/config gilt

# Enter again to forget
t850e9777 = Erneut Enter zum Vergessen

# Nothing matches.
tf8e003c3 = Keine Treffer.

# ↑↓ move   Tab next group   Enter open   Esc close   type > for actions only
t007c4e08 = ↑↓ bewegen   Tab nächste Gruppe   Enter öffnen   Esc schließen   > tippen nur für Aktionen

# ↑↓ preview   Enter keep   Esc go back
t60259b3c = ↑↓ Vorschau   Enter behalten   Esc zurück

# ↑↓ move   Enter twice forget   Esc close   secrets are never shown
tbc202ae0 = ↑↓ bewegen   zweimal Enter vergessen   Esc schließen   Geheimnisse werden nie angezeigt

# Enter copies   Esc close
t39061ccf = Enter kopieren   Esc schließen

# Enter answers   Esc cancels
tbfdc091c = Enter antworten   Esc abbrechen

# ↑↓ move   Enter choose   Esc cancel
te39c67c2 = ↑↓ bewegen   Enter wählen   Esc abbrechen

# General
t5561fd0b = Allgemein

# Appearance
t31018b9f = Darstellung

# Keyboard
t27d1a714 = Tastatur

# SSH and security
t699a801a = SSH und Sicherheit

# Vault
td432914f = Tresor

# About
t58e096de = Über

# Its password (an app password, where it has them)
t22e462e2 = Zugehöriges Passwort (ein App-Passwort, falls angeboten)

# none: the top of the bucket
t863402f7 = keins: oberste Ebene des Buckets

# Its secret
t5709e2db = Zugehöriges Secret

# https://github.com/you/vault.git, or git@github.com:you/vault.git
t367f2cde = https://github.com/you/vault.git, oder git@github.com:you/vault.git

# its default
t7a9ad2ec = Standardwert

# none: git signs in as it does for you
ta76cffea = keins: git meldet sich wie gewohnt an

# any (Gitea: yours)
t97f35ca8 = beliebig (Gitea: deiner)

# choose a folder
tcaf5dc69 = Ordner wählen

# Fill in the address, the user and the password.
t164fba4d = Gib Adresse, Benutzer und Passwort ein.

# The address starts with https://.
t2ed84724 = Die Adresse beginnt mit https://.

# Fill in the endpoint, the bucket, the access key and its secret.
t03a28947 = Gib Endpoint, Bucket, Access Key und dessen Secret ein.

# The endpoint starts with https://.
t64b883b1 = Der Endpoint beginnt mit https://.

# Fill in the repository's address.
t5a34e5a8 = Gib die Adresse des Repositorys ein.

# {shell} (yours)
tb016dc00 = { $shell } (deine)

# e.g. code -n, or /path/to/editor --flag
t455dc834 = z. B. code -n oder /path/to/editor --flag

# Keep logs here
t18df5c77 = Protokolle hier ablegen

# git {version} is installed: Connect again.
t92082827 = git { $version } ist installiert: Klicke erneut auf Verbinden.

# When Apple's installer is done, Connect again.
t2a710b03 = Wenn Apples Installationsprogramm fertig ist, klicke erneut auf Verbinden.

# git was not installed: {e}
t7a63a93e = git wurde nicht installiert: { $e }

# Default ({font})
ta4f51a30 = Standard ({ $font })

# Default (the system's)
te8f5418b = Standard (System)

# Font
ta77a5eb0 = Schrift

# App text
t3b9aa6d3 = App-Text

# Tabs, the host list, the palette, panels and this page.
t494aba4f = Tabs, die Hostliste, die Palette, Panels und diese Seite.

# Size
ta6478e7c = Größe

# The system's
t520575b7 = System

# Custom…
ta391dade = Benutzerdefiniert…

# The file's path is added at the end. Enter saves.
tdd20b80e = Der Pfad der Datei wird am Ende angehängt. Enter speichert.

# As the system ({language})
t4956cc50 = Wie das System ({ $language })

# Language
t9a73db9b = Sprache

# shellwing's own words: menus, Settings, questions and notices. What the terminal shows is the server's. This device's own: it does not sync.
t74b71048 = Die Texte von shellwing: Menüs, Einstellungen, Fragen und Hinweise. Was das Terminal zeigt, kommt vom Server. Gilt nur für dieses Gerät: wird nicht synchronisiert.

# Text editor for remote files
tdc31a941 = Texteditor für entfernte Dateien

# Edit in text editor, in the Files panel, opens a copy with it and writes the file back each time you save. The system's is the desktop's app for plain text.
t3110b5e8 = „Im Texteditor bearbeiten“ im Dateien-Panel öffnet damit eine Kopie und schreibt die Datei bei jedem Speichern zurück. Der des Systems ist die Desktop-App für reinen Text.

# Open the last tabs at start
tf7087aa7 = Beim Start die letzten Tabs öffnen

# When shellwing starts without a host, it opens the tabs and splits it had.
t2b27326b = Startet shellwing ohne Host, öffnet es die Tabs und Teilungen von zuvor.

# These settings, for this device. Edits in the file apply at once.
ta0344de9 = Diese Einstellungen, für dieses Gerät. Änderungen in der Datei gelten sofort.

# Hosts and groups. Synced. No secrets.
t4f70346f = Hosts und Gruppen. Synchronisiert. Keine Geheimnisse.

# Your own themes.
t69ba4c4a = Deine eigenen Designs.

# Passwords and passphrases, encrypted.
te7f92b47 = Passwörter und Passphrasen, verschlüsselt.

# System keychain
t6e83d178 = Schlüsselbund des Systems

# The vault's unlock key.
tdda85be9 = Der Entsperrschlüssel des Tresors.

# Where settings live
tf63ebef5 = Wo Einstellungen liegen

# Open folder
t713a842f = Ordner öffnen

# System
t8f3184bc = System

# Dark
t45570065 = Dunkel

# Light
t8ad2f8cf = Hell

# Block
tbd074002 = Block

# Bar
tdbbc7dba = Balken

# Underline
t9a85011f = Unterstrich

# When scrolling
t3d08a81e = Beim Scrollen

# Terminal text
t77332e04 = Terminaltext

# Line height
t64697aa2 = Zeilenhöhe

# Cursor
t558a502f = Cursor

# Scrollbar
ta18bd919 = Scrollleiste

# Padding at the sides
tb8eb2bd4 = Abstand an den Seiten

# Top and bottom
t56cbbc10 = Oben und unten

# Mode
t534e7732 = Modus

# The system is light now.
t50316f27 = Das System ist gerade hell.

# The system is dark now.
t03050ee1 = Das System ist gerade dunkel.

# Light theme
tf8e348fa = Helles Design

# Dark theme
t5a026490 = Dunkles Design

# The terminal's, while the app is light; the dark mode keeps its own. A host can have its own in its editor.
t0b71dd99 = Das des Terminals, solange die App hell ist; der dunkle Modus behält sein eigenes. Ein Host kann in seinem Editor ein eigenes haben.

# The terminal's, while the app is dark; the light mode keeps its own. A host can have its own in its editor.
te0303bcd = Das des Terminals, solange die App dunkel ist; der helle Modus behält sein eigenes. Ein Host kann in seinem Editor ein eigenes haben.

# Open themes folder
t64678e0b = Designordner öffnen

# A .toml file there is a theme of your own.
t39d886ba = Eine .toml-Datei dort ist ein eigenes Design.

# The app's accent from the theme
t44bdb829 = Akzentfarbe der App aus dem Design

# The logo, what is chosen and the main buttons take the theme's accent color, in place of the app's blue. One too close to the app's background is not taken.
tb5bd008b = Das Logo, die Auswahl und die Hauptschaltflächen übernehmen die Akzentfarbe des Designs statt des App-Blaus. Eine Farbe zu nah am App-Hintergrund wird nicht übernommen.

# {k} k
t450902df = { $k } k

# Reconnect
tcf289b06 = Neu verbinden

# Close
tcd86acc3 = Schließen

# Scrollback
tc94c2be7 = Scrollback

# Lines kept per pane, now {lines}. More uses more memory; new tabs get it.
tc5513217 = Pro Bereich behaltene Zeilen, derzeit { $lines }. Mehr braucht mehr Speicher; gilt für neue Tabs.

# When a connection drops
ted68d834 = Wenn eine Verbindung abbricht

# Reconnect tries again by itself, waiting longer each time. After exit, Enter connects again.
t0b750ede = Neu verbinden versucht es selbst erneut und wartet jedes Mal länger. Nach dem Ende verbindet Enter erneut.

# Local shell
tef7cec6a = Lokale Shell

# What a local terminal runs (This computer, in the host list), with its arguments. Empty: your own shell. Enter saves; new local tabs get it.
t235223ca = Was ein lokales Terminal (Dieser Computer, in der Hostliste) mit seinen Argumenten ausführt. Leer: deine eigene Shell. Enter speichert; gilt für neue lokale Tabs.

# Text
t3e142d5e = Text

# Raw
ta689aa91 = Roh

# Change…
t16f90b63 = Ändern…

# Use the default
tb4d20336 = Standard verwenden

# Session logs
t6a227ff6 = Sitzungsprotokolle

# Log every session
t31c190d6 = Jede Sitzung protokollieren

# From its start, to a file of its own. A host can say otherwise in its editor, and Start Logging in a pane's menu logs it from then on.
t8375a4d9 = Von Beginn an, in eine eigene Datei. Ein Host kann das in seinem Editor anders festlegen, und „Protokoll starten“ im Menü eines Bereichs protokolliert ab dann.

# Format
tffb0ff72 = Format

# Text: the lines as they were left, colors gone; what full-screen programs (vim, htop) draw is left out. Raw: every byte as it came, which cat or less -R shows in color.
taebfec38 = Text: die Zeilen, wie sie stehen blieben, ohne Farben; was Vollbildprogramme (vim, htop) zeichnen, fehlt. Roh: jedes Byte wie empfangen, was cat oder less -R farbig anzeigen.

# The time on each line
t2324a143 = Uhrzeit in jeder Zeile

# Each line of a text log starts with the date and time it was written.
taf5ab76c = Jede Zeile eines Textprotokolls beginnt mit Datum und Uhrzeit ihres Schreibens.

# Folder
t06357afd = Ordner

# Logs go to {folder}. Only you can read them: they hold whatever the sessions showed.
t6395d795 = Protokolle landen in { $folder }. Nur du kannst sie lesen: sie enthalten alles, was die Sitzungen gezeigt haben.

# No folder for logs: choose one.
tc4affc27 = Kein Ordner für Protokolle: wähle einen.

# Command palette
td54729b1 = Befehlspalette

# New tab
td3a5555a = Neuer Tab

# Close the pane
tc4b0686e = Bereich schließen

# Split right
t7c661b21 = Rechts teilen

# Split down
t8ebf2909 = Unten teilen

# Move between panes
td5a160e3 = Zwischen Bereichen wechseln

# {pane}+arrows
tf3ff453e = { $pane }+Pfeiltasten

# Next and previous tab
tdbe14691 = Nächster und vorheriger Tab

# Every tab, by host
td3226d58 = Alle Tabs, nach Host

# Go to tab 1–9
t412f3292 = Zu Tab 1–9 wechseln

# Host list
t2c78c1d9 = Hostliste

# Next and previous theme
t94e69c03 = Nächstes und vorheriges Design

# {chord}+] and [
tcf6edce3 = { $chord }+] und [

# Copy and paste
t2f23e09a = Kopieren und Einfügen

# Back to the bottom
t8f497cc3 = Zurück nach unten

# Window shortcuts never reach the remote host. They cannot be changed yet.
t95c4a480 = Fenster-Tastenkürzel erreichen den entfernten Host nie. Sie lassen sich noch nicht ändern.

# Found. Its keys are tried first.
tc1d15743 = Gefunden. Seine Schlüssel werden zuerst versucht.

# Not found (SSH_AUTH_SOCK is not set).
tb9e9f472 = Nicht gefunden (SSH_AUTH_SOCK ist nicht gesetzt).

# Refuse
t9bcada35 = Ablehnen

# Off
t0dc6b4aa = Aus

# Unknown host keys
t2534790f = Unbekannte Host-Schlüssel

# Ask shows the fingerprint and sends nothing until you accept it. Accept new records it and goes on; Refuse turns it away. A changed key is always refused. A host can have its own in its editor.
t2abc37ff = Fragen zeigt den Fingerabdruck und sendet nichts, bis du ihn akzeptierst. Neue akzeptieren speichert ihn und fährt fort; Ablehnen weist ihn ab. Ein geänderter Schlüssel wird immer abgelehnt. Ein Host kann in seinem Editor eine eigene Einstellung haben.

# A check every so often, so idle connections are not dropped and dead ones are noticed after 3 misses. Now {now}. A host's own (or its ServerAliveInterval) wins.
t1b853e51 = Eine regelmäßige Prüfung, damit ruhende Verbindungen nicht getrennt und tote nach 3 Fehlversuchen erkannt werden. Derzeit { $now }. Die Einstellung eines Hosts (oder sein ServerAliveInterval) hat Vorrang.

# off
tab3a8a0a = aus

# known_hosts file
t2e38efc6 = known_hosts-Datei

# Shared with ssh. shellwing adds the keys you trust.
t43d70395 = Gemeinsam mit ssh genutzt. shellwing fügt die Schlüssel hinzu, denen du vertraust.

# ssh config file
t70767f0d = ssh-Konfigurationsdatei

# Read only. Its hosts show in the host list.
tc5c7d789 = Nur lesend. Ihre Hosts erscheinen in der Hostliste.

# Agent
tf5a4f646 = Agent

# Keys in ssh-agent (Pageant or the OpenSSH agent on Windows).
t28b5b26a = Schlüssel im ssh-agent (Pageant oder der OpenSSH-Agent unter Windows).

# Sync now
ta34dd8fe = Jetzt synchronisieren

# Show the recovery key
t16f62979 = Wiederherstellungsschlüssel anzeigen

# Stop syncing
t59fdbd70 = Synchronisierung beenden

# With {shown}
tabf6bc68 = Mit { $shown }

# Sync with a folder or a server
t6c2be41e = Mit einem Ordner oder Server synchronisieren

# Hosts, saved passwords, keys and snippets travel through a folder another app syncs (iCloud Drive, Syncthing, Dropbox), a WebDAV server or S3-compatible storage. Everything there is encrypted with your recovery key; the server never sees a secret.
tb88756e2 = Hosts, gespeicherte Passwörter, Schlüssel und Snippets laufen über einen Ordner, den eine andere App synchronisiert (iCloud Drive, Syncthing, Dropbox), einen WebDAV-Server oder S3-kompatiblen Speicher. Alles dort ist mit deinem Wiederherstellungsschlüssel verschlüsselt; der Server sieht nie ein Geheimnis.

# Sync settings too
ta578f67d = Auch Einstellungen synchronisieren

# The look (dark or light, themes, fonts), the terminal's and SSH's defaults, and logging. Each device keeps its own sync folder, log folder, local shell and editor.
t12b4e6d2 = Das Aussehen (dunkel oder hell, Designs, Schriften), die Standards von Terminal und SSH sowie die Protokollierung. Jedes Gerät behält seinen eigenen Sync-Ordner, Protokollordner, seine lokale Shell und seinen Editor.

# Let a passphrase join new devices
t6906cf2c = Neue Geräte per Passphrase hinzufügen

# A new device that syncs the same place types it instead of the recovery key or being approved here. It is kept there, sealed by the passphrase: anyone who can read the place can try to guess it, so it has to be hard to guess.
t637858c5 = Ein neues Gerät, das mit demselben Ort synchronisiert, gibt sie ein, statt des Wiederherstellungsschlüssels oder einer Bestätigung hier. Der Schlüssel liegt dort, mit der Passphrase versiegelt: wer den Ort lesen kann, kann versuchen, sie zu erraten, daher muss sie schwer zu erraten sein.

# Hosts kept on this device only (in their editor) stay out of it.
t06079f7b = Hosts, die nur auf diesem Gerät bleiben (in ihrem Editor), sind davon ausgenommen.

# A folder
te84ceb50 = Ein Ordner

# S3-compatible
tdfb04144 = S3-kompatibel

# iCloud Drive, Syncthing, a Dropbox or OneDrive folder, a network share.
tc7eed334 = iCloud Drive, Syncthing, ein Dropbox- oder OneDrive-Ordner, eine Netzwerkfreigabe.

# Choose a folder…
t6268d49f = Ordner wählen…

# Nextcloud, ownCloud, Synology, Koofr, Fastmail: the address of a folder there.
t5f763441 = Nextcloud, ownCloud, Synology, Koofr, Fastmail: die Adresse eines Ordners dort.

# AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi: a bucket, and a key that can read and write it.
t179167f5 = AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi: ein Bucket und ein Access Key mit Lese- und Schreibrechten dafür.

# Endpoint
t27e7d69c = Endpoint

# Region (if it asks for one)
t23e4c7d3 = Region (falls verlangt)

# Bucket
te34e3557 = Bucket

# Folder in the bucket
t4c3f6707 = Ordner im Bucket

# Access key
t837b43b8 = Access Key

# Secret key
t47252d7a = Secret Key

# A private repository on GitHub, GitLab, Gitea or your own server, through this system's git: it signs in as it does for you (its credential manager, your SSH keys and agent). Each change is a commit.
t33e11efe = Ein privates Repository auf GitHub, GitLab, Gitea oder deinem eigenen Server, über das git dieses Systems: es meldet sich an wie bei dir (Credential Manager, deine SSH-Schlüssel und Agent). Jede Änderung ist ein Commit.

# Repository
te763c8b5 = Repository

# Branch
ta3e27665 = Branch

# Token (if git cannot sign in by itself)
t7919b94b = Token (falls git sich nicht selbst anmelden kann)

# The token's user
tb1ad1226 = Benutzer des Tokens

# The password, secret key or token stays on this device, in the vault; other devices sign in with their own.
t2527393b = Passwort, Secret Key oder Token bleiben auf diesem Gerät, im Tresor; andere Geräte melden sich mit ihren eigenen an.

# Installing git…
t77685894 = git wird installiert…

# macOS installs it with Apple's Command Line Tools, in a dialog of its own.
t4d2669ba = macOS installiert es mit Apples Command Line Tools, in einem eigenen Dialog.

# Install git…
te826846c = git installieren…

# Windows installs it with winget, and asks to allow it.
t1f4d87c1 = Windows installiert es mit winget und bittet um Erlaubnis.

# Download git
te5143d09 = git herunterladen

# Install it in a terminal, then Connect again:
t6e62c13a = Installiere es in einem Terminal und klicke dann erneut auf Verbinden:

# Copied.
t53842cb1 = Kopiert.

# Install a newer git (git-scm.com), then Connect again.
t9a84a209 = Installiere ein neueres git (git-scm.com) und klicke dann erneut auf Verbinden.

# None saved yet.
t0c60ecfc = Noch nichts gespeichert.

# {n} saved. | {n} saved.
t2764bcb7 =
    { $n ->
        [one] { $n } gespeichert.
       *[other] { $n } gespeichert.
    }

# The vault is not open.
t3636924b = Der Tresor ist nicht geöffnet.

# Locked: its passphrase or recovery key opens it.
t351c2ed0 = Gesperrt: seine Passphrase oder der Wiederherstellungsschlüssel öffnet ihn.

# Not open.
tcd6d61d0 = Nicht geöffnet.

# In the system keychain, and kept by a passphrase too.
tc986f88d = Im Schlüsselbund des Systems und zusätzlich durch eine Passphrase geschützt.

# Kept by its passphrase: typed when shellwing starts.
t31c62b2e = Durch seine Passphrase geschützt: wird beim Start von shellwing eingegeben.

# In the system keychain.
te1cb09c0 = Im Schlüsselbund des Systems.

# Open, but nothing keeps its key: set a passphrase.
teba6e9a1 = Geöffnet, aber nichts bewahrt seinen Schlüssel: lege eine Passphrase fest.

# Unlock…
tc671ed6b = Entsperren…

# Change the passphrase…
t9653b30a = Passphrase ändern…

# Set a passphrase…
tc40202f8 = Passphrase festlegen…

# Lock now
t1aa78686 = Jetzt sperren

# Ask for it at every start
t1535bdfc = Bei jedem Start abfragen

# Remove the passphrase
tc153db4a = Passphrase entfernen

# Keep it in the system keychain
tc01bbcf0 = Im Schlüsselbund des Systems behalten

# Saved passwords and passphrases
t4b07c4a2 = Gespeicherte Passwörter und Passphrasen

# Offered for saving after you type one that works, and tried before asking next time. Kept in vault.db, encrypted.
ta9eeda13 = Werden zum Speichern angeboten, nachdem du eines eingegeben hast, das funktioniert, und beim nächsten Mal vor der Abfrage versucht. Verschlüsselt in vault.db gespeichert.

# Manage…
t4f2e18b6 = Verwalten…

# The vault's key
td09ac71d = Schlüssel des Tresors

# What opens vault.db. A passphrase keeps it where there is no system keychain (or as well as one); the recovery key always opens it, if the passphrase is forgotten or the keychain loses it.
t6a54a5a7 = Was vault.db öffnet. Eine Passphrase bewahrt ihn, wo es keinen Schlüsselbund des Systems gibt (oder zusätzlich dazu); der Wiederherstellungsschlüssel öffnet ihn immer, falls die Passphrase vergessen ist oder der Schlüsselbund ihn verliert.

# When the vault is locked, {name} opens it, as its passphrase does (which still works). The fingers enrolled now: adding one stops it, until it is turned on again. Needs a signed build of shellwing.
t056f7fb9 = Ist der Tresor gesperrt, öffnet { $name } ihn wie seine Passphrase (die weiterhin funktioniert). Gilt für die jetzt registrierten Finger: Fügst du einen hinzu, wird es deaktiviert, bis du es wieder einschaltest. Erfordert einen signierten Build von shellwing.

# When the vault is locked, {name} (a fingerprint, the face or this device's PIN) opens it, as its passphrase does (which still works). The key is kept by this device's security chip.
t0baa8f35 = Ist der Tresor gesperrt, öffnet { $name } (Fingerabdruck, Gesicht oder die PIN dieses Geräts) ihn wie seine Passphrase (die weiterhin funktioniert). Der Schlüssel liegt im Sicherheitschip dieses Geräts.

# Unlock with {name}
t7c964cc4 = Mit { $name } entsperren

# 1 hour
t0c43fb4a = 1 Stunde

# {m} min
t7eebb6a4 = { $m } Min.

# Lock when idle
ta18da274 = Bei Inaktivität sperren

# After this long without a key pressed or a click, the vault locks until its passphrase is typed. Only while the system keychain does not hold its key: each start asks for the passphrase then.
tf6c5f76d = Nach dieser Zeit ohne Tastendruck oder Klick sperrt sich der Tresor, bis seine Passphrase eingegeben wird. Nur solange der Schlüsselbund des Systems seinen Schlüssel nicht enthält: dann fragt jeder Start nach der Passphrase.

# A native SSH client.
tdb654976 = Ein nativer SSH-Client.

# Open config.toml
t07a52335 = config.toml öffnen

# Filter by name, tag or address
t84bd523e = Nach Name, Tag oder Adresse filtern

# connected
t54eed4d0 = verbunden

# connecting
t5fb60aa7 = verbindet

# connection lost
te76d62e9 = Verbindung verloren

# could not connect
t87553579 = Verbindung fehlgeschlagen

# closed
tebee50c5 = geschlossen

# No hosts yet
ta58f36ff = Noch keine Hosts

# No host matches.
t2a8f8871 = Kein Host passt.

# No hosts yet. Add one, or Host blocks to ~/.ssh/config.
tbbe50baa = Noch keine Hosts. Füge einen hinzu oder Host-Blöcke in ~/.ssh/config.

# + New host
tc7a909aa = + Neuer Host

# + Group
t9585c651 = + Gruppe

# HOSTS
t0d325d34 = HOSTS

# every host
t444612de = alle Hosts

# {n} byte | {n} bytes
te8f231ba =
    { $n ->
        [one] { $n } Byte
       *[other] { $n } Bytes
    }

# {n} MB
t3258fa2a = { $n } MB

# {n} KB
te253ff0c = { $n } KB

# The file's text
t7fb1322b = Der Text der Datei

# What it is, such as kube prod
t2c4b7067 = Was es ist, z. B. kube prod

# It is not text, or longer than {max} KB: replace it from a file instead.
t70f95162 = Sie ist kein Text oder länger als { $max } KB: ersetze sie stattdessen durch eine Datei.

# Where it goes: a path from / or from ~/, such as ~/.kube/config.
t2b706ff4 = Wohin sie kommt: ein Pfad ab / oder ab ~/, z. B. ~/.kube/config.

# Where it goes: a file's path, not a folder's.
t0038f509 = Wohin sie kommt: der Pfad einer Datei, nicht eines Ordners.

# Permissions: three octal digits, such as 600.
t5dde13fd = Berechtigungen: drei Oktalziffern, z. B. 600.

# {size}, not text
t5956f56a = { $size }, kein Text

# Edit…
t8e7efcd7 = Bearbeiten…

# Choose a file…
t28a576c9 = Datei auswählen…

# {file}: {size}, kept as it is
t9e564235 = { $file }: { $size }, unverändert übernommen

# Content
tc1679f62 = Inhalt

# Text up to {edited} is edited here; any file up to {max} can be taken from this computer.
t992520da = Text bis { $edited } wird hier bearbeitet; jede Datei bis { $max } kann von diesem Computer übernommen werden.

# The palette offers to place it in panes of those hosts only.
tdc9acac2 = Die Palette bietet sie nur in Panes dieser Hosts an.

# Where it goes on the host
taf38972d = Wohin sie auf dem Host kommt

# Permissions
t8700fd83 = Berechtigungen

# New config file
td0fa9fed = Neue Konfigurationsdatei

# Edit config file
t56c9aeed = Konfigurationsdatei bearbeiten

# Category
tafb3e591 = Kategorie

# The vault's passphrase, to show the values
teecb1d4f = Passphrase des Tresors, um die Werte anzuzeigen

# No passphrase guards the vault, so anyone at this device can show them. Show the values?
tc7928fa4 = Keine Passphrase schützt den Tresor, also kann jeder an diesem Gerät sie anzeigen. Werte anzeigen?

# value
t425ed3ca = Wert

# What they are for, such as AWS production
tb620b1e5 = Wofür sie gedacht sind, z. B. AWS Produktion

# {name} is not a variable's name.
tdd2c4c4b = { $name } ist kein gültiger Variablenname.

# {name} is there twice.
te1458d84 = { $name } kommt zweimal vor.

# {name} has no value.
t387a8908 = { $name } hat keinen Wert.

# Add a variable: a name and its value.
t6150163b = Füge eine Variable hinzu: einen Namen und seinen Wert.

# The clipboard holds no NAME=value lines.
tbf497c49 = In der Zwischenablage stehen keine NAME=value-Zeilen.

# Not NAME=value: {e}
t65a67e1e = Kein NAME=value: { $e }

# needed by {n} snippet | needed by {n} snippets
t79d75da2 =
    { $n ->
        [one] von { $n } Snippet benötigt
       *[other] von { $n } Snippets benötigt
    }

# Change
ta1de2fdd = Ändern

# The palette offers to load it in panes of those hosts only.
t67715aad = Die Palette bietet an, sie nur in Panes dieser Hosts zu laden.

# Variables
t7996f062 = Variablen

# Add a variable
tf53dee25 = Variable hinzufügen

# Paste a .env file
tf0c57252 = Eine .env-Datei einfügen

# A value can take several lines: Enter starts another, {save} saves.
ta46f6540 = Ein Wert kann mehrere Zeilen haben: Enter beginnt eine neue, { $save } speichert.

# New environment
tf3955dbc = Neue Umgebung

# Edit environment
t79646cbc = Umgebung bearbeiten

# sensitive
ta2bd2f4f = vertraulich

# What it does, to find it in the palette
td9b8e02f = Was es tut, um es in der Palette zu finden

# A name and a command are needed.
t7f4c69bb = Name und Befehl sind erforderlich.

# typed, not run
td4e41905 = eingegeben, nicht ausgeführt

# with {envs}
tba31f3bb = mit { $envs }

# Every host
t77846bfe = Alle Hosts

# A group ▾
tcd864ec5 = Eine Gruppe ▾

# One host ▾
t1864f915 = Ein Host ▾

# Offered for
tbafebe63 = Angeboten für

# No groups yet: make one in the host list.
t31c54e54 = Noch keine Gruppen: erstelle eine in der Hostliste.

# Its variables go from the vault, and from your other devices once they sync. Shells it is loaded in keep them until unloaded.
td2d90383 = Seine Variablen werden aus dem Tresor entfernt und von deinen anderen Geräten, sobald sie synchronisieren. Shells, in die es geladen ist, behalten sie, bis es entladen wird.

# It goes from the vault, and from your other devices once they sync. Where it was placed, it stays.
t9903435f = Sie wird aus dem Tresor entfernt und von deinen anderen Geräten, sobald sie synchronisieren. Wo sie abgelegt wurde, bleibt sie.

# {{host}}, {{user}} and {{address}} are filled in; any other {{name}} is asked for when it runs.
t482910da = {"{{"}host{"}}"}, {"{{"}user{"}}"} und {"{{"}address{"}}"} werden ausgefüllt; jedes andere {"{{"}name{"}}"} wird beim Ausführen abgefragt.

# Asked for when it runs:
t2d8c5e6d = Beim Ausführen abgefragt:

# Enter starts another line; the lines run in turn. {save} saves.
t883a11f4 = Enter beginnt eine neue Zeile; die Zeilen laufen nacheinander. { $save } speichert.

# The palette lists it in panes of those hosts only.
t66336218 = Die Palette zeigt es nur in Bereichen dieser Hosts.

# Environments it needs
tebd9fda9 = Benötigte Umgebungen

# None yet: add one in Settings › Environments.
t769ef9f6 = Noch keine: Füge in Einstellungen › Umgebungen eine hinzu.

# It runs in a subshell that has their variables; they go when it ends.
td5ccd9da = Es läuft in einer Subshell, die ihre Variablen hat; sie verschwinden, wenn sie endet.

# New snippet
tff3f7838 = Neues Snippet

# Edit snippet
t9eab9738 = Snippet bearbeiten

# Command
tc67c8f52 = Befehl

# Run it: press Enter after typing it
t32122ee4 = Ausführen: nach dem Eingeben Enter drücken

# None yet.
t786bc275 = Noch keine.

# New snippet…
tec3111a4 = Neues Snippet…

# New environment…
t0c42d278 = Neue Umgebung…

# New config file…
t58dfdc93 = Neue Konfigurationsdatei…

# New note…
tdad71f9d = Neue Notiz…

# Variables for the shell, such as an API's keys, kept encrypted in the vault and synced with it. Load one into a pane from the palette ({chord}, under Environments) or the pane's menu, and unload it when done; or let a snippet that needs it have it for itself alone.
t7baa8160 = Variablen für die Shell, z. B. die Schlüssel einer API, verschlüsselt im Tresor gespeichert und mit ihm synchronisiert. Lade eine über die Palette ({ $chord }, unter Umgebungen) oder das Menü des Panes in ein Pane und entlade sie wieder, wenn du fertig bist; oder gib sie einem Snippet, das sie braucht, nur für sich allein.

# Commands you run often. Run one from the palette ({chord}, then type its name): Enter runs it in the pane, {alt} in every pane of the tab. Kept in the vault, so they sync with your hosts.
tb1ca142d = Befehle, die du oft ausführst. Führe eines aus der Palette aus ({ $chord }, dann seinen Namen eingeben): Enter führt es im Bereich aus, { $alt } in jedem Bereich des Tabs. Im Tresor gespeichert, daher mit deinen Hosts synchronisiert.

# Free text, kept encrypted in the vault and synced with it: what a server is for, how to restart it, codes to keep. Each opens in a window of its own, from here or the palette ({chord}, under Notes); plain text or Markdown, saved as you type.
t5a3830ee = Freier Text, verschlüsselt im Tresor und mit ihm synchronisiert: wofür ein Server da ist, wie man ihn neu startet, Codes zum Aufbewahren. Jede öffnet sich in einem eigenen Fenster, von hier oder aus der Palette ({ $chord }, unter Notizen); reiner Text oder Markdown, beim Tippen gespeichert.

# Files to place on hosts, such as ~/.kube/config or ~/.aws/credentials, kept encrypted in the vault and synced with it. Place one from the palette ({chord}, under Config files) or the pane's menu: until the tab closes, or to stay.
ta84c0f04 = Dateien, die auf Hosts abgelegt werden, z. B. ~/.kube/config oder ~/.aws/credentials, verschlüsselt im Tresor gespeichert und mit ihm synchronisiert. Lege eine über die Palette ({ $chord }, unter Konfigurationsdateien) oder das Menü des Panes ab: bis der Tab geschlossen wird oder dauerhaft.

# While loaded, the server can read them
te60c8275 = Solange sie geladen sind, kann der Server sie lesen

# Values are sent with the echo off and kept out of the shell's history, but a shell that has them holds them in its memory: whoever has root on that server, or runs as your user there, can read them until they are unloaded or the shell ends.
t87b5db80 = Werte werden mit ausgeschaltetem Echo gesendet und nicht im Verlauf der Shell gespeichert, aber eine Shell, die sie hat, hält sie in ihrem Speicher: Wer auf diesem Server root ist oder dort als dein Benutzer läuft, kann sie lesen, bis sie entladen werden oder die Shell endet.

# Load them on servers you trust, unload them when done, and prefer keys that expire or can do little, such as short-lived tokens or a role with only what the work needs.
t16991c40 = Lade sie nur auf Servern, denen du vertraust, entlade sie, wenn du fertig bist, und bevorzuge Schlüssel, die ablaufen oder wenig können, z. B. kurzlebige Tokens oder eine Rolle mit nur dem, was die Arbeit braucht.

# A placed file is on the server's disk, with its permissions: whoever has root there can read it until it is removed. Place secrets on servers you trust, until the tab closes.
t1d5d16cc = Eine abgelegte Datei liegt auf der Festplatte des Servers, mit ihren Berechtigungen: Wer dort root ist, kann sie lesen, bis sie entfernt wird. Lege Geheimnisse nur auf vertrauenswürdigen Servern ab, und nur bis der Tab geschlossen wird.

# {{host}}, {{user}} and {{address}} are filled in for the pane's host; any other {{name}} is asked for when the snippet runs.
t95cfcc45 = {"{{"}host{"}}"}, {"{{"}user{"}}"} und {"{{"}address{"}}"} werden für den Host des Bereichs ausgefüllt; jedes andere {"{{"}name{"}}"} wird beim Ausführen des Snippets abgefragt.

# Your hosts on every device. Just sign in.
t88dd5f11 = Deine Hosts auf jedem Gerät. Einfach anmelden.

# Hosts, saved passwords, keys and snippets follow you to every computer.
t025d99af = Hosts, gespeicherte Passwörter, Schlüssel und Snippets folgen dir auf jeden Computer.

# Nothing to set up: no folder, no server, no key to type on a new device.
tde7b1d58 = Nichts einzurichten: kein Ordner, kein Server, kein Schlüssel zum Eintippen auf einem neuen Gerät.

# Encrypted on your devices: shellwing never sees your secrets.
t35aecaf3 = Verschlüsselt auf deinen Geräten: shellwing sieht deine Geheimnisse nie.

# Create an account
td7f6c093 = Konto erstellen

# or continue with
t2f60c9bf = oder weiter mit

# Synced · {when}
ta250b3ba = Synchronisiert · { $when }

# With {place}
t6c9fa51a = Mit { $place }

# Set up
t43c06abc = Einrichten

# Syncing…
t8ad2ae06 = Wird synchronisiert…

# Not synced
t4d1a6724 = Nicht synchronisiert

# Paused: the vault is locked
tfff0fb2a = Pausiert: der Tresor ist gesperrt

# With {place}, once it is open
tf75bc464 = Mit { $place }, sobald er geöffnet ist

# {hosts}, {passwords}, {keys} and {snippets}.
t668619a5 = { $hosts }, { $passwords }, { $keys } und { $snippets }.

# {n} host | {n} hosts
t2489674e =
    { $n ->
        [one] { $n } Host
       *[other] { $n } Hosts
    }

# {n} password | {n} passwords
tf07ec59c =
    { $n ->
        [one] { $n } Passwort
       *[other] { $n } Passwörter
    }

# {n} snippet | {n} snippets
t4a5f7c7a =
    { $n ->
        [one] { $n } Snippet
       *[other] { $n } Snippets
    }

# SYNC
t476c685a = SYNC

# Rather not keep it yourself?
t616f184a = Lieber nicht selbst verwalten?

# shellwing cloud keeps it for you: sign in on each device, and that is all. Still end-to-end encrypted.
t81dee99e = shellwing cloud übernimmt das für dich: auf jedem Gerät anmelden, das ist alles. Weiterhin Ende-zu-Ende-verschlüsselt.

# shellwing cloud is coming: signing in opens with it.
t8de35792 = shellwing cloud kommt bald: die Anmeldung startet damit.

# {n} host · $2.99/mo | {n} hosts · $2.99/mo
t6036c21e =
    { $n ->
        [one] { $n } Host · $2.99/Monat
       *[other] { $n } Hosts · $2.99/Monat
    }

# unlimited · $5.99/mo
td7d2332f = unbegrenzt · $5.99/Monat

# A folder, WebDAV, S3 or Git of your own:
t42ad626a = Ein eigener Ordner, WebDAV, S3 oder Git:

# Settings › Sync
t6790eaeb = Einstellungen › Sync

# just now
ta715ef0f = gerade eben

# a minute ago
t85ac693d = vor einer Minute

# {n} minute ago | {n} minutes ago
t6c7302d6 =
    { $n ->
        [one] vor { $n } Minute
       *[other] vor { $n } Minuten
    }

# an hour ago
t84377069 = vor einer Stunde

# {n} hour ago | {n} hours ago
t137b052e =
    { $n ->
        [one] vor { $n } Stunde
       *[other] vor { $n } Stunden
    }

# {n} day ago | {n} days ago
tb9fa7744 =
    { $n ->
        [one] vor { $n } Tag
       *[other] vor { $n } Tagen
    }

# its password is not saved on this device: connect again in Settings › Sync
td2fdfc60 = sein Passwort ist auf diesem Gerät nicht gespeichert: verbinde dich erneut unter Einstellungen › Sync

# hosts not synced: {e}
te7d5212e = Hosts nicht synchronisiert: { $e }

# settings not synced: {e}
tf8592ea2 = Einstellungen nicht synchronisiert: { $e }

# settings not written: {e}
t60f438b7 = Einstellungen nicht geschrieben: { $e }

# hosts not written: {e}
t00f3a4eb = Hosts nicht geschrieben: { $e }

# Find a tab: host, user, title
t187bd0b5 = Tab suchen: Host, Benutzer, Titel

# {n} tab | {n} tabs
t09a24eca =
    { $n ->
        [one] { $n } Tab
       *[other] { $n } Tabs
    }

# {n} pane | {n} panes
tb53e7b62 =
    { $n ->
        [one] { $n } Bereich
       *[other] { $n } Bereiche
    }

# this tab
tfd01d10a = dieser Tab

# Tab {number}
te154cf6f = Tab { $number }

# No tab matches.
t9b01e94e = Kein Tab passt.

# ↑↓ choose
t983339a6 = ↑↓ wählen

# Enter go to it
td90b12ab = Enter dorthin wechseln

# Cmd+W close it
t6246d272 = Cmd+W schließen

# Ctrl+W close it
t82a6adaf = Ctrl+W schließen

# A full-screen program is running in this pane: leave it, then try again.
t20301272 = In diesem Pane läuft ein Vollbildprogramm: Beende es und versuche es dann erneut.

# Removed {n} file. | Removed {n} files.
t0f179fa4 =
    { $n ->
        [one] { $n } Datei entfernt.
       *[other] { $n } Dateien entfernt.
    }

# It has no variables.
t0993a534 = Sie hat keine Variablen.

# With the ones loaded, that comes to {size} KB; a shell takes {max} KB at most: unload one first.
t3883bd87 = Mit den geladenen sind das { $size } KB; eine Shell nimmt höchstens { $max } KB: Entlade zuerst eine.

# The shell did not take the variables: it is not at its prompt, or not a POSIX shell (sh, bash, zsh).
ta1a9b215 = Die Shell hat die Variablen nicht übernommen: Sie ist nicht am Prompt oder keine POSIX-Shell (sh, bash, zsh).

# {name} is loaded into this shell.
t5bc879d1 = { $name } ist in diese Shell geladen.

# Log saved: {path}
tcc43a8d9 = Protokoll gespeichert: { $path }

# Logging to {path}
td2342a94 = Protokolliert in { $path }

# Cannot log: {e}
tb673f869 = Protokollieren nicht möglich: { $e }

# Saved as HTML: {path}
t21558f46 = Als HTML gespeichert: { $path }

# Saved as text: {path}
t17977298 = Als Text gespeichert: { $path }

# logging
t897ecb9e = protokolliert

# listening
t77104bea = lauscht

# left out
t73aeedd4 = ausgelassen

# Installing {key} on {host}…
t9e6d8462 = { $key } wird auf { $host } installiert…

# the connection ended
t36d02558 = die Verbindung wurde beendet

# {key} is installed on {host}.
t4ae79143 = { $key } ist auf { $host } installiert.

# {host} has {key} already.
t12a2b3c2 = { $host } hat { $key } bereits.

# {key} was not installed on {host}: {why}
t25610553 = { $key } wurde nicht auf { $host } installiert: { $why }

# {key} is installed on {host}. Sign in to {host} with it from now on?
ta04e62fb = { $key } ist auf { $host } installiert. Ab jetzt damit bei { $host } anmelden?

# Not now
t2c3d6fce = Nicht jetzt

# Use it for {host}
t704df588 = Für { $host } verwenden

# Find
t3e1cd17a = Suchen

# Invalid pattern
t5b9dcaca = Ungültiges Muster

# No results
tdd4785bc = Keine Ergebnisse

# {current} of {total}{more}
ta6206114 = { $current } von { $total }{ $more }

# {total} found{more}
t59fb4764 = { $total } gefunden{ $more }

# Match case (Alt+C)
t692d685e = Groß-/Kleinschreibung beachten (Alt+C)

# Regular expression (Alt+R)
tc1b10302 = Regulärer Ausdruck (Alt+R)

# The Files panel browses a server over SFTP; this pane is a shell on this computer.
t0e83f1c9 = Das Dateien-Panel durchsucht einen Server per SFTP; dieser Bereich ist eine Shell auf diesem Computer.

# Save the password for {target} in the vault?
tceb19137 = Passwort für { $target } im Tresor speichern?

# Save the passphrase of {key_path} in the vault?
tc75d3456 = Passphrase von { $key_path } im Tresor speichern?

# Saved in the vault.
t0d69c4e6 = Im Tresor gespeichert.

# The host key for {host} has changed.
t1e510d66 = Der Host-Schlüssel von { $host } hat sich geändert.

# If the server was reinstalled or its key replaced, trust the new key. If you do not know why it changed, do not: someone may be between you and the server, reading what you send.
t26f8b8a1 = Wenn der Server neu installiert oder sein Schlüssel ersetzt wurde, vertraue dem neuen Schlüssel. Wenn du nicht weißt, warum er sich geändert hat, tu es nicht: jemand könnte zwischen dir und dem Server sitzen und mitlesen, was du sendest.

# Known
t3f42d62c = Bekannt

# {old}  (known_hosts line {line})
t8797a1b8 = { $old }  (known_hosts Zeile { $line })

# Now
t8b3dc597 = Jetzt

# Trust the new key and connect
t48aebd90 = Neuem Schlüssel vertrauen und verbinden

# Not trusted: {e}
tdfcff622 = Nicht vertraut: { $e }

# Connection lost. Trying again in {seconds} s (attempt {attempt} of {attempts}).
t63f1b6c3 = Verbindung verloren. Neuer Versuch in { $seconds } s (Versuch { $attempt } von { $attempts }).

# Gave up after {n} attempt. Enter tries again. | Gave up after {n} attempts. Enter tries again.
t446dfb2c =
    { $n ->
        [one] Nach { $n } Versuch aufgegeben. Enter versucht es erneut.
       *[other] Nach { $n } Versuchen aufgegeben. Enter versucht es erneut.
    }

# Connection lost. Enter tries again.
t1ca41f48 = Verbindung verloren. Enter versucht es erneut.

# {why}. Enter starts it again.
tcad1f44f = { $why }. Enter startet es neu.

# {why}. Enter reconnects.
t9788f089 = { $why }. Enter verbindet neu.

# Could not connect. Enter tries again.
tfbba91f8 = Verbindung fehlgeschlagen. Enter versucht es erneut.

# Stop (Esc)
t47aaed65 = Stoppen (Esc)

# Start again (Enter)
tbfd489be = Neu starten (Enter)

# Reconnect (Enter)
t06029ac3 = Neu verbinden (Enter)

# Font size {size}
t3fc7bcbc = Schriftgröße { $size }

# The pasted text could run commands. Paste again within 5 s to send it anyway.
t6e9aeddb = Der eingefügte Text könnte Befehle ausführen. Füge innerhalb von 5 s erneut ein, um ihn trotzdem zu senden.

# Logging stopped: {e}
t435e7ac0 = Protokollierung gestoppt: { $e }

# Command Palette…
t14aa6ff7 = Befehlspalette…

# Snippets…
t2b3c5b61 = Snippets…

# Load Environment…
td34b2cc0 = Umgebung laden…

# Place Config File…
t49f82f0c = Konfigurationsdatei ablegen…

# Keep a File in the Vault…
t25c07703 = Datei im Tresor speichern…

# Remove {names}
t4efeb00b = { $names } entfernen

# Unload {names}
t362b9a48 = { $names } entladen

# Paste
tb34a0288 = Einfügen

# Find…
t1ecb565a = Suchen…

# Hide Files
t55bc30ae = Dateien ausblenden

# Browse Files (SFTP)
tf41b1da0 = Dateien durchsuchen (SFTP)

# Forward a Port…
te7550adc = Port weiterleiten…

# Save Output as Text…
tffbf1682 = Ausgabe als Text speichern…

# Save Output as HTML…
t30c151b0 = Ausgabe als HTML speichern…

# with colors
te0d866bb = mit Farben

# Stop Logging
td281e470 = Protokoll beenden

# Start Logging
t72c41b76 = Protokoll starten

# Split Right
t50db06c1 = Rechts teilen

# Split Down
t1165de29 = Unten teilen

# Close Pane
t21a947cf = Bereich schließen

# ↓ Back to bottom (Shift+End)
t5a2b7304 = ↓ Zurück nach unten (Shift+End)

# Config files · {category}
t59edc3e9 = Konfigurationsdateien · { $category }

# placed: Enter places it again
taf8f6d41 = abgelegt: Enter legt sie erneut ab

# {name} · {size} · permissions {mode}
ta22bb64e = { $name } · { $size } · Berechtigungen { $mode }

# Where on {host}
t0f76d445 = Wohin auf { $host }

# Cannot place {name}: {e}
t1383f29b = { $name } kann nicht abgelegt werden: { $e }

# Replace the one placed here until the tab closes
t5ca1c37e = Die hier abgelegte ersetzen, bis der Tab geschlossen wird

# What was there before still comes back when the tab closes.
t55130ddc = Was vorher dort war, kommt beim Schließen des Tabs zurück.

# {path} holds it already.
t3ec4172b = { $path } enthält sie bereits.

# Place it until the tab closes
t19537c52 = Ablegen, bis der Tab geschlossen wird

# Then it is taken away.
t1302797a = Danach wird sie entfernt.

# Place it to stay
t698a38cf = Dauerhaft ablegen

# It stays when the tab closes.
tb2bc8f3b = Sie bleibt, wenn der Tab geschlossen wird.

# Replace the file there until the tab closes
t7e646019 = Die Datei dort ersetzen, bis der Tab geschlossen wird

# The one there now is set aside, and comes back when the tab closes.
t8c121382 = Die jetzige wird beiseitegelegt und kommt beim Schließen des Tabs zurück.

# Replace it, keeping the old one
tc95f7291 = Ersetzen und die alte behalten

# As {path}
tfb10a76a = Als { $path }

# Replace it
t1e17fc0c = Ersetzen

# The one there now is gone.
t7fa127b9 = Die jetzige ist dann weg.

# {path} is there already…
td716b366 = { $path } existiert bereits…

# Place at {path}…
tdcc97a76 = Unter { $path } ablegen…

# Placed {name} at {path} until the tab closes.
t59328aae = { $name } unter { $path } abgelegt, bis der Tab geschlossen wird.

# Placed {name} at {path}.
tfce33e2b = { $name } unter { $path } abgelegt.

# Took away {n} config file left here from before. | Took away {n} config files left here from before.
ta674430e =
    { $n ->
        [one] { $n } Konfigurationsdatei entfernt, die von früher übrig war.
       *[other] { $n } Konfigurationsdateien entfernt, die von früher übrig waren.
    }

# Left {path} as it is: it changed since it was placed.
tccbb9d92 = { $path } unverändert gelassen: Sie wurde seit dem Ablegen geändert.

# Kept encrypted as a config file, for this host, up to {max} KB.
tf7742770 = Verschlüsselt als Konfigurationsdatei für diesen Host gespeichert, bis zu { $max } KB.

# A file on {host} to keep in the vault
t63904f7b = Eine Datei auf { $host }, die im Tresor gespeichert werden soll

# Kept {path} in the vault: see Settings › Config files.
t8589664d = { $path } im Tresor gespeichert: siehe Einstellungen › Konfigurationsdateien.

# Not kept: {e}
tc12e4203 = Nicht abgelegt: { $e }

# Removing the config files placed…
tbd123099 = Die abgelegten Konfigurationsdateien werden entfernt…

# Open a tab first
t6460fddf = Öffne zuerst einen Tab

# Connect first
t0b55252f = Verbinde dich zuerst

# Notes for {host}
ta77bd589 = Notizen für { $host }

# Notes · {category}
tbb112cdf = Notizen · { $category }

# Over SSH only
t14e43465 = Nur über SSH

# Snippets · {category}
t85f58d76 = Snippets · { $category }

# Enter · Alt+Enter: {n} pane | Enter · Alt+Enter: {n} panes
t3bbceccc =
    { $n ->
        [one] Enter · Alt+Enter: { $n } Bereich
       *[other] Enter · Alt+Enter: { $n } Bereiche
    }

# Open a tab first: a snippet runs in the focused pane.
t45260fd2 = Öffne zuerst einen Tab: Ein Snippet läuft im ausgewählten Bereich.

# in this pane
t57139bb8 = in diesem Bereich

# in {n} pane | in {n} panes
tca97c422 =
    { $n ->
        [one] in { $n } Bereich
       *[other] in { $n } Bereichen
    }

# {snippet} · {panes} · {name} ({at} of {total})
t6748f755 = { $snippet } · { $panes } · { $name } ({ $at } von { $total })

# Will run
teaaf5750 = Wird ausgeführt

# An environment this needs was deleted: edit the snippet.
tb37e7456 = Eine Umgebung, die dieses Snippet braucht, wurde gelöscht: Bearbeite das Snippet.

# loaded: Enter loads it again
t1a97d76a = geladen: Mit Enter wird sie erneut geladen

# Split the tab first: there is one pane to type in.
t72699bd1 = Teile zuerst den Tab: es gibt nur einen Bereich zum Tippen.

# Close the tab?
tc626ae98 = Tab schließen?

# Close the pane?
tf2b43783 = Bereich schließen?

# The connection to {one} is open; closing ends it.
t7958d376 = Die Verbindung zu { $one } ist offen; Schließen beendet sie.

# {n} connection is open; closing ends it. | {n} connections are open; closing ends them.
tbac763bb =
    { $n ->
        [one] { $n } Verbindung ist offen; Schließen beendet sie.
       *[other] { $n } Verbindungen sind offen; Schließen beendet sie.
    }

# Cannot open a terminal: {e}
t3ffc1c98 = Terminal kann nicht geöffnet werden: { $e }

# A connection is open; quitting closes it.
tf3732b18 = Eine Verbindung ist offen; Beenden schließt sie.

# {n} connection is open; quitting closes it. | {n} connections are open; quitting closes them.
td41065cb =
    { $n ->
        [one] { $n } Verbindung ist offen; Beenden schließt sie.
       *[other] { $n } Verbindungen sind offen; Beenden schließt sie.
    }

# Quit
tc7b3d356 = Beenden

# Quit shellwing?
tfa31f588 = shellwing beenden?

# Choose a host on the left to connect.
tf7759ebb = Wähle links einen Host zum Verbinden.

# Unlock the vault…
tdf659182 = Tresor entsperren…

# Snippets, environments and config files are in it.
t1646a763 = Snippets, Umgebungen und Konfigurationsdateien sind darin.

# Another tab to {title}
ta46c00f0 = Weiterer Tab zu { $title }

# Split right, to {title}
tef0930dc = Rechts teilen, zu { $title }

# Split down, to {title}
tfe6ae2e4 = Unten teilen, zu { $title }

# Close the tab
t1c6c1637 = Tab schließen

# Stop typing in every pane
tfa86f71a = Nicht mehr in alle Bereiche tippen

# Type in every pane of this tab
t047bdf97 = In alle Bereiche dieses Tabs tippen

# All {count} tabs, by host…
t4780291e = Alle { $count } Tabs, nach Host…

# Find in {title}…
t9f86553f = In { $title } suchen…

# Save the output of {title} as text…
t8947128e = Ausgabe von { $title } als Text speichern…

# Save the output of {title} as HTML, with colors…
t1c2d8b8c = Ausgabe von { $title } als HTML mit Farben speichern…

# Stop logging {title}
t87730020 = Protokoll von { $title } beenden

# Log {title} to a file
t2f1b2b25 = { $title } in eine Datei protokollieren

# Hide the files
td567c539 = Dateien ausblenden

# Files on {title}
t13c5bc91 = Dateien auf { $title }

# Forward a port through {title}…
t9f033387 = Port über { $title } weiterleiten…

# Unload environment: {names}
tbe80f7b5 = Umgebung entladen: { $names }

# Change theme…
t85a401b4 = Design ändern…

# Hide the host list
td7560c88 = Hostliste ausblenden

# Show the host list
t4754a8f9 = Hostliste einblenden

# Saved passwords and passphrases…
tdfc17c92 = Gespeicherte Passwörter und Passphrasen…

# Lock the vault
tc05449a1 = Tresor sperren

# Sync now with {place}
t8aad97ad = Jetzt mit { $place } synchronisieren

# Sync passwords with a folder…
t74de6b57 = Passwörter mit einem Ordner synchronisieren…

# Sync passwords with a server…
tc73a9bf6 = Passwörter mit einem Server synchronisieren…

# Settings…
t64d0d0b4 = Einstellungen…

# New host…
t13806b73 = Neuer Host…

# New group…
t2c0113fa = Neue Gruppe…

# Edit host {name}…
te6b404e6 = Host { $name } bearbeiten…

# Edit hosts.toml
tbfa1762c = hosts.toml bearbeiten

# Edit config.toml
tf108a14d = config.toml bearbeiten

# No snippet for this host: add one in Settings › Snippets.
tdd2847bb = Kein Snippet für diesen Host: Füge in Einstellungen › Snippets eines hinzu.

# No config file for this host: add one in Settings › Config files.
t4b38a37d = Keine Konfigurationsdatei für diesen Host: Füge in Einstellungen › Konfigurationsdateien eine hinzu.

# No environment for this host: add one in Settings › Environments.
t1b95315b = Keine Umgebung für diesen Host: Füge in Einstellungen › Umgebungen eine hinzu.

# This host uses the theme `{t}` from hosts.toml; change it there.
td55512c8 = Dieser Host nutzt das Design `{ $t }` aus hosts.toml; ändere es dort.

# Synced {n} change from other devices. | Synced {n} changes from other devices.
tffe9e6a0 =
    { $n ->
        [one] { $n } Änderung von anderen Geräten synchronisiert.
       *[other] { $n } Änderungen von anderen Geräten synchronisiert.
    }

# Sync: {e}
t8cb8c8c3 = Sync: { $e }

# The other device refused to let this one in.
t1d4ed983 = Das andere Gerät hat dieses abgelehnt.

# No device let this one in within 10 minutes. Ask again, or use the recovery key.
t410f845d = Kein Gerät hat dieses innerhalb von 10 Minuten zugelassen. Frage erneut oder nutze den Wiederherstellungsschlüssel.

# Pairing: {e}
te7df26f8 = Kopplung: { $e }

# Let {device} into this vault?
tfab743ab = { $device } in diesen Tresor lassen?

# It gets everything the vault holds: passwords, keys, snippets and hosts. Approve only if {device} shows the code {code}.
tc6029096 = Es erhält alles, was der Tresor enthält: Passwörter, Schlüssel, Snippets und Hosts. Bestätige nur, wenn { $device } den Code { $code } zeigt.

# Approve
te6edf55a = Bestätigen

# The vault is locked
t78b7a6ee = Der Tresor ist gesperrt

# Its passphrase opens it, or its recovery key (13 groups of four). Esc leaves it locked: Unlock the vault, in the palette, asks again.
t5eb207c8 = Seine Passphrase öffnet ihn, oder sein Wiederherstellungsschlüssel (13 Vierergruppen). Esc lässt ihn gesperrt: „Tresor entsperren…“ in der Palette fragt erneut.

# The system keychain lost the vault's key
t444b9525 = Der Schlüsselbund des Systems hat den Schlüssel des Tresors verloren

# Its recovery key (13 groups of four) opens it, and puts the key back in the keychain.
ta628efeb = Sein Wiederherstellungsschlüssel (13 Vierergruppen) öffnet ihn und legt den Schlüssel wieder in den Schlüsselbund.

# {name} is asking
ta753cd6b = { $name } fragt an

# Or type the passphrase here.
tb78be92d = Oder gib hier die Passphrase ein.

# The vault's passphrase, or its recovery key
t81044cdb = Passphrase des Tresors oder sein Wiederherstellungsschlüssel

# The recovery key
tbaa8dc18 = Der Wiederherstellungsschlüssel

# The vault is open.
t3773d56a = Der Tresor ist geöffnet.

# Not opened that way: {e}
t7693053c = Auf diese Weise nicht geöffnet: { $e }

# Type it again
ta4c19926 = Erneut eingeben

# To be sure of it.
te7d01782 = Zur Sicherheit.

# There is no system keychain to keep the vault's key
t04970b55 = Es gibt keinen Schlüsselbund des Systems für den Schlüssel des Tresors

# A passphrase keeps it instead: you type it when shellwing starts. At least {min} characters. Esc: no saved passwords, keys or sync this time.
t2fd0f1b3 = Stattdessen bewahrt ihn eine Passphrase: du gibst sie beim Start von shellwing ein. Mindestens { $min } Zeichen. Esc: diesmal keine gespeicherten Passwörter, Schlüssel oder Synchronisierung.

# A passphrase for the vault
t25841dd8 = Eine Passphrase für den Tresor

# It opens the vault where the system keychain cannot. At least {min} characters.
tf7ebf632 = Sie öffnet den Tresor, wo der Schlüsselbund des Systems es nicht kann. Mindestens { $min } Zeichen.

# A passphrase
tf2500b30 = Eine Passphrase

# That does not open it: {e}.
tdcd34ef8 = Das öffnet ihn nicht: { $e }.

# The vault stays locked: {e}
t05ea3b39 = Der Tresor bleibt gesperrt: { $e }

# Too short: at least {min} characters.
tff7b3675 = Zu kurz: mindestens { $min } Zeichen.

# The two were not the same.
t60aece29 = Die beiden stimmen nicht überein.

# Recovery key
t02904a59 = Wiederherstellungsschlüssel

# Enter copies
t4cc7b1ae = Enter kopiert

# Write this key down and keep it safe.
t86925e9b = Schreib diesen Schlüssel auf und bewahre ihn sicher auf.

# It opens the vault if the passphrase is forgotten; Settings › Vault shows it again.
t1de04861 = Er öffnet den Tresor, falls die Passphrase vergessen ist; Einstellungen › Tresor zeigt ihn erneut.

# The vault is ready; its passphrase keeps it.
t41eb3235 = Der Tresor ist bereit; seine Passphrase schützt ihn.

# The passphrase opens the vault too now.
tb612b2c2 = Die Passphrase öffnet jetzt auch den Tresor.

# No passphrase set: {e}
t41c038bf = Keine Passphrase festgelegt: { $e }

# A passphrase that joins new devices
tde75a705 = Eine Passphrase, mit der neue Geräte beitreten

# Typed on a new device that syncs the same folder, instead of the recovery key. Anyone who can read the folder can try to guess it: four or more unrelated words, at least {min} characters.
t703339bb = Wird auf einem neuen Gerät, das denselben Ordner synchronisiert, statt des Wiederherstellungsschlüssels eingegeben. Wer den Ordner lesen kann, kann versuchen, sie zu erraten: vier oder mehr unzusammenhängende Wörter, mindestens { $min } Zeichen.

# A sync passphrase
ta0a73805 = Eine Sync-Passphrase

# {place} lets a passphrase join its vault
t2ac4c55f = { $place } erlaubt den Beitritt zum Tresor per Passphrase

# The one set on a device that syncs (Settings › Sync). Secrets saved here are kept and merged.
t14a40721 = Die auf einem synchronisierenden Gerät festgelegte (Einstellungen › Sync). Hier gespeicherte Geheimnisse bleiben erhalten und werden zusammengeführt.

# The sync passphrase
t9fb2f5f5 = Die Sync-Passphrase

# Too easy to guess (strength {strength} of 4; 3 is needed). Several unrelated words work well.
tb51562c6 = Zu leicht zu erraten (Stärke { $strength } von 4; 3 nötig). Mehrere unzusammenhängende Wörter eignen sich gut.

# Not set: this device does not sync.
t3aadb1e5 = Nicht festgelegt: dieses Gerät synchronisiert nicht.

# Setting the sync passphrase…
tbf6d3ccf = Sync-Passphrase wird festgelegt…

# New devices can join with the sync passphrase now.
t2bd178bc = Neue Geräte können jetzt mit der Sync-Passphrase beitreten.

# Not set: {e}
t8229a0e7 = Nicht festgelegt: { $e }

# Joining…
taa884c51 = Tritt bei…

# Cannot join: {e}
t6d9d6813 = Beitritt nicht möglich: { $e }

# Approve it on a device that syncs
t70c052c8 = Auf einem synchronisierenden Gerät bestätigen

# That device asks whether to let this one in; you check both show the same code.
tb533a5e4 = Das andere Gerät fragt, ob es dieses zulassen soll; prüfe, ob beide denselben Code zeigen.

# Type the recovery key
te9ca27fa = Wiederherstellungsschlüssel eingeben

# The 13 groups of four from when syncing started.
t23f96b77 = Die 13 Vierergruppen vom Start der Synchronisierung.

# Type the sync passphrase
t8cd53b17 = Sync-Passphrase eingeben

# The one a device that syncs set to let new devices join.
taf23bf2b = Die von einem synchronisierenden Gerät festgelegte, damit neue Geräte beitreten können.

# {folder} holds a vault from another device: join it
t3ef84906 = { $folder } enthält einen Tresor von einem anderen Gerät: beitreten

# Code {code}
t0e7c074b = Code { $code }

# {device} asks whether to let this device in. Approve there if it shows this same code.
t24c15ff5 = { $device } fragt, ob dieses Gerät zugelassen werden soll. Bestätige dort, wenn es denselben Code zeigt.

# Waiting for a device that syncs
t6e41520f = Warte auf ein synchronisierendes Gerät

# Open shellwing on a device that syncs this folder: it asks whether to let this one in. Esc cancels.
ted2b04ee = Öffne shellwing auf einem Gerät, das diesen Ordner synchronisiert: es fragt, ob es dieses zulassen soll. Esc bricht ab.

# Sync with this folder
t8eb0599a = Mit diesem Ordner synchronisieren

# it is still opening
tdc19bf2a = er wird noch geöffnet

# Cannot sync: the vault: {why}.
tad5847da = Synchronisierung nicht möglich: der Tresor: { $why }.

# Cannot keep the password: {e}
tcf7fa2ce = Passwort kann nicht gespeichert werden: { $e }

# Connecting to {place}…
tea99befd = Verbinde mit { $place }…

# Cannot sync: {e}
td3aecb42 = Synchronisierung nicht möglich: { $e }

# The vault: {why}.
t775cecf8 = Der Tresor: { $why }.

# Joined: this device syncs with {place}.
t8992145b = Beigetreten: dieses Gerät synchronisiert mit { $place }.

# Cannot save where the vault syncs: {e}
t7eec807c = Kann nicht dort speichern, wo der Tresor synchronisiert: { $e }

# {folder} holds a vault from another device.
tccddb8cb = { $folder } enthält einen Tresor von einem anderen Gerät.

# Type its recovery key to share it. Secrets saved here are kept and merged.
t7915bff9 = Gib seinen Wiederherstellungsschlüssel ein, um ihn zu teilen. Hier gespeicherte Geheimnisse bleiben erhalten und werden zusammengeführt.

# That recovery key does not open it.
t2418fa69 = Dieser Wiederherstellungsschlüssel öffnet ihn nicht.

# Check each group of four; letters and case do not need to be exact.
t6a984230 = Prüfe jede Vierergruppe; Buchstaben und Groß-/Kleinschreibung müssen nicht exakt sein.

# Recovery key: XXXX-XXXX-…
t931d6f73 = Wiederherstellungsschlüssel: XXXX-XXXX-…

# Connected
t7bf69370 = Verbunden

# Other hosts · connected first
t63758b2c = Andere Hosts · verbundene zuerst

# Install {key} on…
t80b2a794 = { $key } installieren auf…

# {key} goes on here once connected.
t02f786c3 = { $key } wird nach dem Verbinden hier installiert.

# no config directory
t33e2c7cb = kein Konfigurationsverzeichnis

# {host} signs in with {key} from now on.
t054dfae0 = { $host } meldet sich ab jetzt mit { $key } an.

# Cannot edit hosts.toml: {e}
t917924da = hosts.toml kann nicht bearbeitet werden: { $e }

# Saved {name}.
t966c9f6d = { $name } gespeichert.

# Saved the group {name}.
t58507cbd = Gruppe { $name } gespeichert.

# Deleted the group {name}.
t2dbe70db = Gruppe { $name } gelöscht.

# Port 8080 here reaches localhost:80 as the server sees it.
t51796bc3 = Port 8080 hier erreicht localhost:80 aus Sicht des Servers.

# Port 9000 on the server reaches localhost:3000 here.
t79226dc9 = Port 9000 auf dem Server erreicht localhost:3000 hier.

# A SOCKS proxy on port 1080 here; connections go out from the server.
t8be5905e = Ein SOCKS-Proxy auf Port 1080 hier; Verbindungen gehen vom Server aus.

# For this tab, until it is closed
tfe049b9e = Für diesen Tab, bis er geschlossen wird

# To forward on every connection, add it to the host's forward list in hosts.toml.
te97c1f00 = Um bei jeder Verbindung weiterzuleiten, füge es der Weiterleitungsliste des Hosts in hosts.toml hinzu.

# Syncing. Write this key down and keep it safe.
t967ce868 = Synchronisierung läuft. Schreib diesen Schlüssel auf und bewahre ihn sicher auf.

# The key of the synced vault.
te0a56fe7 = Der Schlüssel des synchronisierten Tresors.

# Another device needs it to join; it also opens the vault if this device is lost.
ta32e11fd = Ein anderes Gerät braucht ihn zum Beitreten; er öffnet auch den Tresor, falls dieses Gerät verloren geht.

# the vault is still opening
t0cf7277a = der Tresor wird noch geöffnet

# No saved passwords: {why}.
tf97d3647 = Keine gespeicherten Passwörter: { $why }.

# Password for {target}
t2cd4c302 = Passwort für { $target }

# Enter to forget
t7512451f = Enter zum Vergessen

# Passphrase of {key_path}
t328fcceb = Passphrase von { $key_path }

# Cannot read the vault: {e}
taee646ba = Tresor nicht lesbar: { $e }

# Nothing saved yet: after a password works, the pane offers to save it.
t586cd916 = Noch nichts gespeichert: wenn ein Passwort funktioniert, bietet der Bereich an, es zu speichern.

# Pairing cancelled.
t9bf01ace = Kopplung abgebrochen.

# Forgotten.
te8ff7bb9 = Vergessen.

# Theme: {name}
ted8446cd = Design: { $name }

# Cannot read the key: {e}
t744465b3 = Schlüssel nicht lesbar: { $e }

# the system keychain does not hold the vault's key: without a passphrase, only the recovery key would open it
td554dbfe = der Schlüsselbund des Systems enthält den Schlüssel des Tresors nicht: ohne Passphrase würde nur der Wiederherstellungsschlüssel ihn öffnen

# The vault opens with the system keychain alone now.
t5426fdee = Der Tresor öffnet sich jetzt allein mit dem Schlüsselbund des Systems.

# Passphrase kept: {e}.
tcc4fe4d2 = Passphrase beibehalten: { $e }.

# Presence
t834965a4 = Anwesenheit

# {name} opens the vault now, as its passphrase does.
t30f109d1 = { $name } öffnet jetzt den Tresor, wie seine Passphrase.

# {name} no longer opens the vault.
t92f8fdac = { $name } öffnet den Tresor nicht mehr.

# The vault is locked.
tc91b8040 = Der Tresor ist gesperrt.

# The vault cannot lock: the system keychain opens it, or it has no passphrase.
t2e6801fd = Der Tresor kann nicht gesperrt werden: der Schlüsselbund des Systems öffnet ihn, oder er hat keine Passphrase.

# The vault's key is out of the system keychain: each start asks for the passphrase.
tc8dd0fb9 = Der Schlüssel des Tresors ist nicht mehr im Schlüsselbund des Systems: jeder Start fragt nach der Passphrase.

# The key stays in the keychain: {e}.
tdbb24cfc = Der Schlüssel bleibt im Schlüsselbund: { $e }.

# New devices join with the recovery key, or by approving them.
t4b5eba19 = Neue Geräte treten mit dem Wiederherstellungsschlüssel bei oder durch Bestätigung.

# Not stopped: {e}
t3ea0b584 = Nicht beendet: { $e }

# The system keychain keeps the vault's key now.
tbdfd4327 = Der Schlüsselbund des Systems bewahrt jetzt den Schlüssel des Tresors.

# Stopped syncing. What is there and the secrets here are kept.
t4ba0ed42 = Synchronisierung beendet. Was dort liegt und die Geheimnisse hier bleiben erhalten.

# Cannot create the file: {e}
te92446ce = Datei kann nicht erstellt werden: { $e }

# No config directory.
t16df9b1f = Kein Konfigurationsverzeichnis.

# The vault locked: {n} minute without use. | The vault locked: {n} minutes without use.
t9abc648a =
    { $n ->
        [one] Der Tresor wurde gesperrt: { $n } Minute ohne Nutzung.
       *[other] Der Tresor wurde gesperrt: { $n } Minuten ohne Nutzung.
    }

# Passwords cannot be saved: {p}
t1ce3459e = Passwörter können nicht gespeichert werden: { $p }

# this computer
t7d16e2ca = dieser Computer

# Local terminal
t50537e20 = Lokales Terminal

# Typing goes to {listening} of {n} pane. Click a pane's badge to leave it out or bring it back. | Typing goes to {listening} of {n} panes. Click a pane's badge to leave it out or bring it back.
t3023d2ae =
    { $n ->
        [one] Eingaben gehen an { $listening } von { $n } Bereich. Klicke auf das Badge eines Bereichs, um ihn auszulassen oder zurückzuholen.
       *[other] Eingaben gehen an { $listening } von { $n } Bereichen. Klicke auf das Badge eines Bereichs, um ihn auszulassen oder zurückzuholen.
    }

# Stop ({keys})
t6ae11828 = Stoppen ({ $keys })

# This computer
tfa0d176a = Dieser Computer

# Favorites
t7c2c53ba = Favoriten

# read only
t0d7ec241 = nur lesend

# Choose a host on the left to connect
t8354c717 = Wähle links einen Host zum Verbinden

# or press {keys} to find one, or to connect to user@host
t4f67c373 = oder drücke { $keys }, um einen zu suchen oder mit user@host zu verbinden

# find a host, connect, run an action
t4b795631 = Host suchen, verbinden, Aktion ausführen

# a new tab
t8ed35e75 = neuer Tab

# close the tab
teb891a97 = Tab schließen

# next tab
ta7702de7 = nächster Tab

# go to a tab
tf70ede40 = zu einem Tab wechseln

# show or hide the host list
t9489dcc2 = Hostliste ein- oder ausblenden
