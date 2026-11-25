# shellwing in pt. Each message is found by its English, the
# comment above it; scripts/i18n.py keeps this file in step with the code.

# waiting for you
t49268f5c = aguardando você

# running
t147eb74c = em execução

# exit 0
tf83c441d = código 0

# exit {code}
te0c7c2fc = código { $code }

# ended by a signal
t450a0cd5 = encerrado por um sinal

# did not run
tf05d1c4a = não executado

# stopped
t6ee2f34e = parado

# refused by you
t2fd484b3 = recusado por você

# All
t75afe364 = Todos

# Waiting for you
te8233afc = Aguardando você

# Running
t452925ec = Em execução

# Failed
t5579c3d4 = Com falha

# No AI client has run a command yet.
td29d28d8 = Nenhum cliente de AI executou um comando ainda.

# Nothing here.
ta7006e9a = Nada aqui.

# Stop all AI
tddf9c9cc = Parar toda a AI

# {client} · {host} · started {when} · {job}
tafbed557 = { $client } · { $host } · iniciado às { $when } · { $job }

# {client} wants to write this on {host}. It waits for you: where risky ones wait, every write does.
t2b63bd77 = { $client } quer gravar isto em { $host }. A gravação espera por você: onde os arriscados esperam, toda gravação espera.

# {client} wants to run this on {host}. It waits for you: its grant asks for every command.
t2a306810 = { $client } quer executar isto em { $host }. O comando espera por você: a concessão dele pede confirmação para cada comando.

# {client} wants to run this on {host}. It waits for you: it has {words}, from the risky list.
t85e6d49e = { $client } quer executar isto em { $host }. O comando espera por você: ele contém { $words }, da lista de palavras arriscadas.

# Run it
t72430853 = Executar

# Run it, and this command again this session
t2efebee7 = Executar, e este comando de novo nesta sessão

# Refuse it
tbea0b112 = Recusar

# Stop
t4b7f7705 = Parar

# Copy output
t64749f61 = Copiar saída

# Run this once in a terminal:
t2a18fdaf = Execute isto uma vez em um terminal:

# In Claude Desktop, Settings › Developer › Edit Config, add this to claude_desktop_config.json:
td57f67eb = No Claude Desktop, em Settings › Developer › Edit Config, adicione isto a claude_desktop_config.json:

# Add this to {file}:
t314ece69 = Adicione isto a { $file }:

# In Cline, MCP Servers › Configure, add this to cline_mcp_settings.json:
t0ff991db = No Cline, em MCP Servers › Configure, adicione isto a cline_mcp_settings.json:

# A client that starts its MCP server itself, over stdio:
td4ff0be4 = Um cliente que inicia o próprio servidor MCP, via stdio:

# Run freely
t77cb7aab = Executar livremente

# Commands run at once. Each is kept in AI activity, as it runs.
t1bfe0ae2 = Os comandos rodam na hora. Cada um fica em Atividade de AI, enquanto roda.

# Ask for risky ones
td43721d0 = Perguntar nos arriscados

# A command with words from the list below waits for you; the rest run.
t846e6005 = Um comando com palavras da lista abaixo espera por você; os demais rodam.

# Ask every time
t26ab736e = Perguntar sempre

# Each command waits until you allow it, or allow it for the session.
tdcef213b = Cada comando espera até você permiti-lo, ou permiti-lo para a sessão.

# Find a host
t5f515591 = Encontre um servidor

# A command with these words waits
t1678b953 = Um comando com estas palavras espera

# AI clients
ta302119f = Clientes de AI

# Agents that reach your hosts through shellwing, over MCP. They never see a key, a password or a passphrase: shellwing signs in for them.
t6c56bef3 = Agentes que alcançam seus servidores pelo shellwing, via MCP. Eles nunca veem uma chave, uma senha ou uma frase secreta: o shellwing entra por eles.

# Add client
td53111fb = Adicionar cliente

# Which client?
t278d359e = Qual cliente?

# No client yet: Add client lets one in.
t6815a21c = Nenhum cliente ainda: Adicionar cliente permite a entrada de um.

# token …{end}
t4a145d1b = token …{ $end }

# paused
t0a1997cb = pausado

# connected now
t432c5c88 = conectado agora

# not connected
tb83f675d = não conectado

# Resume
t454d1d8a = Retomar

# Pause
t44f9bf2d = Pausar

# Config
t6a12e0e7 = Configuração

# Remove
t21a5901d = Remover

# Pause stops a client for now, and keeps its token and the hosts given it. Remove takes its token back.
t00a3399a = Pausar interrompe um cliente por enquanto e mantém seu token e os servidores concedidos a ele. Remover revoga seu token.

# (the token shown when it was added)
t9375386f = (o token mostrado quando o cliente foi adicionado)

# Copy
t658f3664 = Copiar

# The token shows this once: shellwing keeps only its hash. Remove takes it back.
t6ebf1609 = O token aparece só esta vez: o shellwing guarda apenas o hash dele. Remover o revoga.

# The token is not kept, only its hash: to see one again, remove the client and add it again.
tf69e6847 = O token não é guardado, só o hash dele: para ver um de novo, remova o cliente e adicione-o de novo.

# Hosts given
t1ef9da69 = Servidores concedidos

# A client reaches only these. Its commands run in the background, on a connection of their own, and each is kept in AI activity.
t396f69c1 = Um cliente alcança só estes. Os comandos dele rodam em segundo plano, numa conexão própria, e cada um fica em Atividade de AI.

# AI activity
t07e2f764 = Atividade de AI

# Take every one back
tb3911aec = Revogar todos

# Give hosts…
t30b956e9 = Conceder servidores…

# No host is given to an AI client.
t654696aa = Nenhum servidor foi concedido a um cliente de AI.

# {n} host | {n} hosts
t2489674e =
    { $n ->
        [one] { $n } servidor
       *[other] { $n } servidores
    }

# {n} paused | {n} paused
t7fb84985 =
    { $n ->
        [one] { $n } pausado
       *[other] { $n } pausados
    }

# Client paused
te9192f58 = Cliente pausado

# Take all back
t4a2d0a34 = Revogar todos

# in the background
t04874b67 = em segundo plano

# through a tab
t6bca7a6e = por uma aba

# reads files
te3f10929 = lê arquivos

# writes files
t0a7257de = grava arquivos

# while shellwing stays open
t938960ff = enquanto o shellwing estiver aberto

# until taken back
tba10b16b = até ser revogado

# ended
t1a30bf3b = encerrado

# for {n} min more
t955674cd = por mais { $n } min

# Paused
tde281feb = Pausado

# Take back
t0650c681 = Revogar

# Clear group
tc2447199 = Desmarcar grupo

# Select all
t6af21072 = Selecionar todos

# given already
t0912be5e = já concedido

# No host chosen yet.
t99637c8e = Nenhum servidor escolhido ainda.

# {n} host chosen | {n} hosts chosen
tf9b0b9a0 =
    { $n ->
        [one] { $n } servidor escolhido
       *[other] { $n } servidores escolhidos
    }

# Clear
t04a57fc2 = Limpar

# Until I take it back
taacfb8db = Até eu revogar

# While shellwing stays open
tdb1d179f = Enquanto o shellwing estiver aberto

# 1 hour
t0c43fb4a = 1 hora

# 8 hours
t63affd14 = 8 horas

# Give access
te96c4c6e = Conceder acesso

# Give access to {n} host | Give access to {n} hosts
t58ec7fe6 =
    { $n ->
        [one] Conceder acesso a { $n } servidor
       *[other] Conceder acesso a { $n } servidores
    }

# Hosts
tcf9aefb4 = Servidores

# To
t41f59644 = Para

# For
t0f7e1b30 = Por

# How
t644e442d = Modo

# In the background
t206da987 = Em segundo plano

# On a connection of its own, with no tab. Each command and its output are in AI activity.
t4353fb5e = Numa conexão própria, sem aba. Cada comando e a saída dele ficam em Atividade de AI.

# Through a tab
tfe61838e = Por uma aba

# Typed into a tab to the host, in its shell, where you see it; a tab opens if none is. A password the host asks for there is yours to type.
ta797f7de = Digitados numa aba do servidor, no shell dele, onde você os vê; uma aba se abre se não houver nenhuma. Uma senha que o servidor pedir ali é você quem digita.

# It may
t5c846351 = Pode

# Run commands
t65b79874 = Executar comandos

# and see what they print
tef4e221a = e ver o que eles imprimem

# Read files
te1736866 = Ler arquivos

# list folders and read files, over SFTP
tf3b3e2e7 = listar pastas e ler arquivos, via SFTP

# Write files
tc9796e43 = Gravar arquivos

# over SFTP; where risky ones wait, every write waits for you
t2d9c8c34 = via SFTP; onde os arriscados esperam, toda gravação espera por você

# Commands
t971550f3 = Comandos

# Given to {client} already: {hosts}. Giving them again replaces what they had.
tebca595f = Já concedidos a { $client }: { $hosts }. Concedê-los de novo substitui o que tinham.

# It never sees a key, a password or a passphrase: a host that would ask for one is refused, as is one whose key is not known yet. A host's output can carry words meant to steer an agent: give hosts to one you trust with them.
t7620726b = Ele nunca vê uma chave, uma senha ou uma frase secreta: um servidor que pediria uma é recusado, assim como um cuja chave ainda não é conhecida. A saída de um servidor pode trazer palavras feitas para direcionar um agente: conceda servidores apenas a um cliente a quem você os confiaria.

# Cancel
t35afca3b = Cancelar

# When AI runs a command
tc94fc353 = Quando a AI executa um comando

# For hosts given from now on; each can be set apart in the table above. Every command is kept in AI activity, whichever this is.
t608375b2 = Para servidores concedidos daqui em diante; cada um pode ter a própria configuração na tabela acima. Todo comando fica em Atividade de AI, seja qual for esta opção.

# Risky: a command with any of these, a line each, waits for you
te371b0dd = Arriscados: um comando com qualquer uma destas, uma por linha, espera por você

# The usual list
tcc58d242 = A lista de sempre

# unlock its windows
t7c69953e = desbloquear suas janelas

# Confirm it is you to unlock shellwing.
t31ace1f1 = Confirme que é você para desbloquear o shellwing.

# Checking…
t87f62cb7 = Verificando…

# Not confirmed: shellwing stays locked.
t1db04c44 = Não confirmado: o shellwing continua bloqueado.

# That is not the password.
t06b4bdb7 = Essa não é a senha.

# Cannot confirm it is you: {e}
t0a90c137 = Não é possível confirmar que é você: { $e }

# Windows Hello or your Windows password unlocks it.
tfe68d2e0 = O Windows Hello ou sua senha do Windows o desbloqueia.

# Touch ID or your Mac's password unlocks it.
tdd855245 = O Touch ID ou a senha do seu Mac o desbloqueia.

# Your login password unlocks it.
t8eeb5228 = Sua senha de login o desbloqueia.

# Unlock
t9c799975 = Desbloquear

# Quit shellwing
t8857c967 = Sair do shellwing

# shellwing is locked
t665c7b6c = O shellwing está bloqueado

# Your login password
t2348e42c = Sua senha de login

# shellwing is locked: unlock it in its main window.
tf4ae07e3 = O shellwing está bloqueado: desbloqueie-o na janela principal dele.

# Daylight
t29cc3b47 = Luz do dia

# Paper
tfea8bfd9 = Papel

# Soft gray
t6d3b9d7c = Cinza suave

# High contrast
tdc2a9fd1 = Alto contraste

# Mist
td071b28c = Névoa

# Sage
t5152348d = Sálvia

# Lavender
t40f6ad0e = Lavanda

# Sand
t3e40cd83 = Areia

# Dusk
t44655688 = Crepúsculo

# Neutral
te9330778 = Neutro

# Graphite
tcc5f9dad = Grafite

# Midnight
tfc321421 = Meia-noite

# Slate
t9637e38e = Ardósia

# Dim
t35691dd9 = Atenuado

# Deep sea
t372a2e54 = Mar profundo

# Espresso
t77f4d515 = Espresso

# Forest
t953ca480 = Floresta

# Plum
t1d8d478b = Ameixa

# An edit of {remote} was not written back to the server; the file is kept as {path}.
t62769e5b = Uma edição de { $remote } não foi gravada de volta no servidor; o arquivo foi mantido como { $path }.

# {n} edited file was not written back to the server; it is kept in Downloads: {files}. | {n} edited files were not written back to the server; they are kept in Downloads: {files}.
td8a345bd =
    { $n ->
        [one] { $n } arquivo editado não foi gravado de volta no servidor; ele foi mantido em Downloads: { $files }.
       *[other] { $n } arquivos editados não foram gravados de volta no servidor; eles foram mantidos em Downloads: { $files }.
    }

# no editor command
tbf2a5d3a = nenhum comando de editor

# no text editor found
t6256b3da = nenhum editor de texto encontrado

# Name
t0fe07306 = Nome

# The connection ended.
tc85117e2 = A conexão foi encerrada.

# The connection ended while writing back.
t424caf2a = A conexão foi encerrada durante a gravação de volta.

# Not connected.
tc9ec8929 = Não conectado.

# Cannot make a folder to edit in: {err}
t4d82727e = Não é possível criar uma pasta para edição: { $err }

# the text editor
t91cdab0c = o editor de texto

# Cannot start {who}: {e}
tc39185d1 = Não é possível iniciar { $who }: { $e }

# Not connected: written back once it is.
t8345869d = Não conectado: será gravado de volta assim que conectar.

# Not written back: {e}
t6b172035 = Não gravado de volta: { $e }

# Cannot move it to Downloads: {e}
t33407575 = Não é possível movê-lo para Downloads: { $e }

# not written back to the server: discard your changes?
t1adcdad0 = não gravado de volta no servidor: descartar suas alterações?

# opening…
t185c4e41 = abrindo…

# in the editor; each save is written back
tcf8122af = no editor; cada salvamento é gravado de volta

# written back at {t}
t4a0a44e2 = gravado de volta às { $t }

# writing back…
ta2088cca = gravando de volta…

# changed on the server since you opened it
tf8056821 = alterado no servidor desde que você o abriu

# not on the server; moved to Downloads as {name}
tbc889b90 = não está no servidor; movido para Downloads como { $name }

# Open
t538b10e9 = Abrir

# Discard
t1e706c21 = Descartar

# Keep in Downloads
t3052af2e = Manter em Downloads

# Show
t29742e5c = Mostrar

# Overwrite
t78ff0e92 = Sobrescrever

# Retry
t8036af59 = Tentar de novo

# Upload
t826f7f7c = Enviar

# Not deleted: {err}
t74196840 = Não excluído: { $err }

# Edit in text editor
td8d39fb8 = Editar no editor de texto

# Edit in {editor}
te44cd57b = Editar em { $editor }

# Open with default app
t5ef80ec5 = Abrir com o app padrão

# Download
t6964bb19 = Baixar

# Download As…
t955e4fd1 = Baixar como…

# New folder…
t4f4860f1 = Nova pasta…

# Upload…
tb19d9b38 = Enviar…

# Refresh
t0815aad4 = Atualizar

# Copy path
tf165cb2b = Copiar caminho

# Rename…
t8628af0d = Renomear…

# Delete…
t2fed07aa = Excluir…

# link
t0ddb0669 = link

# {percent}% · {rate}/s
td8a3a0d4 = { $percent }% · { $rate }/s

# starting…
t1a0ebe8f = iniciando…

# already there
te2f6c72d = já existe

# Merge
t8b725bc7 = Mesclar

# Replace
te4d54f63 = Substituir

# Skip
tc197c7e2 = Pular

# done · {size}
t38ca2d3b = concluído · { $size }

# Files
te71d1b30 = Arquivos

# Home
t52f50eae = Início

# New folder
t1926d127 = Nova pasta

# Rename {old}
t2a3708da = Renomear { $old }

# Enter to save · Esc to cancel
t9abaa44e = Enter para salvar · Esc para cancelar

# Delete the folder {name} and everything in it?
t219d4be7 = Excluir a pasta { $name } e tudo o que há nela?

# Delete {name}?
t4397aaa4 = Excluir { $name }?

# Delete
t5797ea6a = Excluir

# Opening SFTP…
t6c4d75a4 = Abrindo SFTP…

# Loading…
tc4e2f181 = Carregando…

# Try again
t213e90fa = Tentar de novo

# Empty folder.
t56515154 = Pasta vazia.

# Right-click a file or folder for its actions; drop files here to upload them.
t1377d961 = Clique com o botão direito em um arquivo ou pasta para ver as ações; solte arquivos aqui para enviá-los.

# Production
t89cb4f3a = Produção

# A name is needed.
tad1c356c = É preciso um nome.

# Another group has this name.
tc0f2f206 = Outro grupo tem este nome.

# There is no config directory to save in.
t24dff34a = Não há diretório de configuração para salvar.

# This group cannot be found in hosts.toml.
tbc9872ca = Este grupo não foi encontrado em hosts.toml.

# New group
tb7b3f75a = Novo grupo

# Group in hosts.toml
tfc009326 = Grupo em hosts.toml

# Not saved yet
t8662a2eb = Ainda não salvo

# Same as the app
tf3e994b9 = Igual ao app

# Theme for its hosts
t9d045818 = Tema dos seus servidores

# None
t304ff7fb = Nenhum

# Certificates from
te7ecc3f7 = Certificados de

# No hosts yet. Choose this group in a host's editor.
t73e08796 = Nenhum servidor ainda. Escolha este grupo no editor de um servidor.

# Choose the group in a host's editor to put the host in it.
t880fbefc = Escolha o grupo no editor de um servidor para colocar o servidor nele.

# {n} host: {names} | {n} hosts: {names}
t79084a5c =
    { $n ->
        [one] { $n } servidor: { $names }
       *[other] { $n } servidores: { $names }
    }

# Group
t0574902c = Grupo

# Color
te5b43cf8 = Cor

# A host's own theme wins over its group's.
tad991bdd = O tema próprio de um servidor prevalece sobre o do grupo.

# Its hosts sign in with a certificate from this organization on your shellwing server, good for a few hours. Its admins choose the accounts you may log in as.
t441237fe = Os servidores do grupo entram com um certificado desta organização no seu servidor shellwing, válido por algumas horas. Os administradores dela escolhem as contas com que você pode entrar.

# Delete {title}?
ta14ea5b3 = Excluir { $title }?

# Delete {title}? Its {n} host stay. | Delete {title}? Its {n} hosts stay.
t7eb2e426 =
    { $n ->
        [one] Excluir { $title }? Seu { $n } servidor permanece.
       *[other] Excluir { $title }? Seus { $n } servidores permanecem.
    }

# Save
t4d2d5d68 = Salvar

# Automatic
t616a6784 = Automático

# Key file
t6ed7c612 = Arquivo de chave

# Password
t2cc30838 = Senha

# Agent only
t80e1a754 = Somente agente

# optional
t49c5d8f9 = opcional

# 10.0.0.5 or host.example.com
t685963cf = 10.0.0.5 ou host.example.com

# your login name
t62d0236a = seu nome de login

# a host here, an ssh alias, or user@host
tdcf5544b = um servidor daqui, um alias do ssh ou user@host

# Staging
tcb81d7c6 = Homologação

# Search icons
t024abc2b = Buscar ícones

# Another host has this name.
te27e50cd = Outro servidor tem este nome.

# No spaces.
t02933ecd = Sem espaços.

# Another host uses this.
t946bf8e8 = Outro servidor usa isto.

# An address is needed.
t140a0aeb = É preciso um endereço.

# 1 to 65535.
t1bbc0d91 = De 1 a 65535.

# Choose a key.
t6e8ece2a = Escolha uma chave.

# Seconds, 0 to 3600 (0: none).
tc0567af7 = Segundos, de 0 a 3600 (0: nenhum).

# A host cannot jump through itself.
t272071b9 = Um servidor não pode saltar por si mesmo.

# Jump hosts or a command: one of them, not both.
t1a129586 = Servidores de salto ou um comando: um dos dois, não ambos.

# new key
t2aaac45a = nova chave

# the vault is not open
t97a4b927 = o cofre não está aberto

# No key made: {e}
t4e14e42b = Nenhuma chave criada: { $e }

# A full path, such as /root/.ssh/id_ed25519.
ta80b048d = Um caminho completo, como /root/.ssh/id_ed25519.

# Its passphrase
t279e8bbb = A frase secreta dela

# That passphrase does not open it.
t05c6a9c1 = Essa frase secreta não a abre.

# {path} is not an OpenSSH private key.
te16bf944 = { $path } não é uma chave privada OpenSSH.

# Not copied: {e}
t03370a6b = Não copiada: { $e }

# Waiting for the password…
tc4be2aeb = Aguardando a senha…

# Add to the vault
t236cfbb2 = Adicionar ao cofre

# Read as administrator
t44c7c72e = Ler como administrador

# Key file, read as administrator
t023daf07 = Arquivo de chave, lido como administrador

# The system asks for an administrator's password, once. The key then goes into the vault as one of your own: from then on your vault keeps it, not the file's owner, and it syncs to your other devices.
tfd88dc14 = O sistema pede a senha de um administrador uma vez. Depois a chave vai para o cofre como uma das suas: a partir daí o seu cofre a guarda, não o dono do arquivo, e ela é sincronizada com seus outros dispositivos.

# This host cannot be found in hosts.toml.
ta0e18527 = Este servidor não foi encontrado em hosts.toml.

# You may connect to the hosts of {name}, not change them.
t90a5554d = Você pode se conectar aos servidores de { $name }, mas não alterá-los.

# This host's collection is no longer synced here.
tdd3d6aa3 = A coleção deste servidor não é mais sincronizada aqui.

# {name} copy
t0d616a81 = cópia de { $name }

# Choose
t73c66160 = Escolher

# Chosen here: the host is not asked which system it runs.
tcef39ba0 = Escolhido aqui: não se pergunta ao servidor qual sistema ele executa.

# Automatic: {system}
td31d5fc9 = Automático: { $system }

# Seen on the host, which is asked again at most once a day.
tfafb596b = Informado pelo servidor, que é consultado de novo no máximo uma vez por dia.

# The host would not say which system it runs. It is asked again at most once a day.
t65649443 = O servidor não informou qual sistema executa. Ele é consultado de novo no máximo uma vez por dia.

# Shows the system the host runs, once it has connected.
t1cc0e6ad = Mostra o sistema que o servidor executa, depois que ele se conectar.

# Icon
t68e6cb10 = Ícone

# Change
ta1de2fdd = Alterar

# Systems
t39f411dd = Sistemas

# Cloud
tc0821b1e = Nuvem

# Themes
tbf419633 = Temas

# No icon matches.
t23c804ac = Nenhum ícone corresponde.

# host name
t8465975e = nome do servidor

# Reaches
t9a71395c = Alcança

# Key
tcd1ac90c = Chave

# Through
t2e38a41a = Via

# Forwards
t5567756b = Redirecionamentos

# It could not be read.
t54886f1b = Não foi possível lê-lo.

# From ~/.ssh/config
t85f5968b = De ~/.ssh/config

# Host from ~/.ssh/config
t6a7dc843 = Servidor de ~/.ssh/config

# shellwing reads this file but never writes to it. Edit it in your editor, or make a copy here to add a group, a theme, forwards or a saved password.
t509e342f = O shellwing lê este arquivo, mas nunca grava nele. Edite-o no seu editor ou faça uma cópia aqui para adicionar um grupo, um tema, redirecionamentos ou uma senha salva.

# Open config file
t54ea8995 = Abrir arquivo de configuração

# Copy to my hosts
t72c32fa6 = Copiar para meus servidores

# New host
t5c18d08d = Novo servidor

# Saved in hosts.toml
teae7befe = Salvo em hosts.toml

# Connection
t09ea4ff9 = Conexão

# Alias for the command line
ted5013c8 = Alias para a linha de comando

# Address
t700560f3 = Endereço

# Port
te2c53cc6 = Porta

# User
te0a63e12 = Usuário

# Sign in
t744be623 = Entrar

# {name} · in the vault · {kind}
tbf1c044e = { $name } · no cofre · { $kind }

# Choose a key
t32d1fb40 = Escolha uma chave

# {name}   in the vault · {kind}
ta6d50f9b = { $name }   no cofre · { $kind }

# Another key file…
t6f4497c1 = Outro arquivo de chave…

# Key file as administrator…
t7c2b0195 = Arquivo de chave como administrador…

# + Generate a key for this host
ta1a39e38 = + Gerar uma chave para este servidor

# + Generate a key for {host}
t2065d130 = + Gerar uma chave para { $host }

# the host
t39dac2c6 = o servidor

# {name} is new: {host} does not have it yet. Saving and installing signs in once another way (a password, say), then adds it.
t7ecf0d6d = { $name } é nova: { $host } ainda não a tem. Salvar e instalar entra uma vez de outra forma (com uma senha, por exemplo) e depois a adiciona.

# Save and install on {host}
t1707b1d3 = Salvar e instalar em { $host }

# This key is in your own vault: the others with access to {name} cannot sign in with it. Choose a key file they have, a password, or the organization's certificates.
t123028f6 = Esta chave está no seu próprio cofre: os outros com acesso a { $name } não conseguem entrar com ela. Escolha um arquivo de chave que eles tenham, uma senha ou os certificados da organização.

# Tries the agent, then your keys in ~/.ssh and the vault, then asks for a password, like ssh.
t786a9bff = Tenta o agente, depois suas chaves em ~/.ssh e no cofre, depois pede uma senha, como o ssh.

# Kept in the vault: no passphrase to ask for.
t74ee0f69 = Guardada no cofre: nenhuma frase secreta a pedir.

# Passphrase: asked for when needed, and can be saved in the vault.
tbdfc0bf6 = Frase secreta: pedida quando necessário, e pode ser salva no cofre.

# Asked for when connecting, and can be saved in the vault.
ta6140863 = Pedida ao conectar, e pode ser salva no cofre.

# Uses the keys in your ssh-agent.
t3a4ba988 = Usa as chaves do seu ssh-agent.

# {name} (from the group)
t5d454f47 = { $name } (do grupo)

# Same as the group
t046bf19d = Igual ao grupo

# A certificate from the organization is tried first, before the way above.
t07c38387 = Um certificado da organização é tentado primeiro, antes do método acima.

# + Add jump
t3f09cb93 = + Adicionar salto

# Route
td7d630be = Rota

# Jump through, in order
tfc8a9b1f = Saltar por, nesta ordem

# Enter adds what you typed · Esc stops adding
ta13c2c20 = Enter adiciona o que você digitou · Esc para de adicionar

# Or through a command
t7ba76a07 = Ou por um comando

# Its input and output carry the connection, as ssh's ProxyCommand: %h, %p, %r and %n are the host, port, user and name. It runs on computers, not phones.
t011a46a2 = A entrada e a saída dele levam a conexão, como o ProxyCommand do ssh: %h, %p, %r e %n são o servidor, a porta, o usuário e o nome. Ele roda em computadores, não em celulares.

# + Add forward
tcfdf6ef4 = + Adicionar redirecionamento

# Port forwarding
tb033f949 = Redirecionamento de portas

# Set up every time this host connects.
t0d74aaf9 = Configurado sempre que este servidor conecta.

# Sync
t89a71bba = Sincronização

# Keep on this device only
t20b0191f = Manter somente neste dispositivo

# Not synced: other devices do not see this host.
t657130a6 = Não sincronizado: outros dispositivos não veem este servidor.

# Synced with your other devices when sync is on.
tb80f1c8f = Sincronizado com seus outros dispositivos quando a sincronização está ativa.

# {n} (this host only)
tb6562d26 = { $n } (somente este servidor)

# Organize
t6ddb17ec = Organizar

# My hosts
tcb6506f0 = Meus servidores

# Saved in
tf3fe99c3 = Salvo em

# Everybody with access to the collection sees it, end-to-end encrypted.
t4152ce3a = Todos com acesso à coleção o veem, com criptografia de ponta a ponta.

# Only you see it, on your devices.
t6f22a0a3 = Só você o vê, nos seus dispositivos.

# Saving moves it from {from} to {to}: it is deleted for everybody with access to {from}.
t4c0827d7 = Salvar o move de { $from } para { $to }: ele é excluído para todos com acesso a { $from }.

# Saving moves it from {from} to {to}.
taf4375c8 = Salvar o move de { $from } para { $to }.

# In the collection {name}: everybody with access to it sees it.
t98d877a2 = Na coleção { $name }: todos com acesso a ela o veem.

# Tags
t76bda0c0 = Tags

# + New group…
t9d11ac85 = + Novo grupo…

# New group's name
t2e4cb82f = Nome do novo grupo

# Made when you save this host.
taf695c2c = Criado quando você salvar este servidor.

# Shown on the tab and the pane border
t090fc8f1 = Mostrada na aba e na borda do painel

# Favorite: listed first in the host list
t527b0c5e = Favorito: listado primeiro na lista de servidores

# Terminal
t93c562d1 = Terminal

# Theme
t4eaf7c92 = Tema

# Run after connecting
t2b3c50ce = Executar após conectar

# Typed into the shell once it is ready, after every connect and reconnect.
t2ac048fc = Digitado no shell assim que ele estiver pronto, após cada conexão e reconexão.

# App setting
t4e528c40 = Configuração do app

# Ask
t7cfd61b2 = Perguntar

# Accept new
ta6e1de9b = Aceitar novas

# Strict
t6d561894 = Estrito

# Always
tad4bbd04 = Sempre

# Never
tb2ff2b29 = Nunca

# none
tada7afdb = nenhum

# {s} s
te78896af = { $s } s

# Advanced
t60f3de7b = Avançado

# Unknown host key
t57114866 = Chave de servidor desconhecida

# The app's setting is {app_key} now. A changed key is always refused.
t6669805b = A configuração do app agora é { $app_key }. Uma chave alterada é sempre recusada.

# Session log
t54c8d181 = Registro da sessão

# Always: each session is logged to a file from its start. Never: not even when the app logs every session. The app logs {logs} now.
t9ccf92c6 = Sempre: cada sessão é registrada em um arquivo desde o início. Nunca: nem quando o app registra todas as sessões. O app agora registra { $logs }.

# every session
t4dbfb628 = todas as sessões

# Keepalive
t3d5b35a9 = Keepalive

# Seconds between checks; 0: none. Empty: the app's setting ({app_alive}).
t66f5835d = Segundos entre verificações; 0: nenhuma. Vazio: a configuração do app ({ $app_alive }).

# Forward my agent to this host
t80664148 = Encaminhar meu agente para este servidor

# While you are connected, whoever is root on this host can sign in elsewhere with your keys too. Only for hosts you trust.
t741afb89 = Enquanto você estiver conectado, quem for root neste servidor também pode entrar em outros lugares com suas chaves. Somente para servidores em que você confia.

# From this host you could then git push or ssh further with the keys on this computer; they are never copied there.
t01f5f399 = A partir deste servidor, você poderá fazer git push ou ssh adiante com as chaves deste computador; elas nunca são copiadas para lá.

# X11 windows
td33b1f40 = Janelas X11

# As ~/.ssh/config
tff363503 = Como ~/.ssh/config

# Off
t0dc6b4aa = Desativado

# Untrusted
tb69519d9 = Não confiável

# Trusted
t102c70dc = Confiável

# Lets programs on the host open windows here, on an X server. Untrusted ones are kept from your other windows and keyboard; trusted ones are not, so only for hosts you trust.
t07477a92 = Permite que programas do servidor abram janelas aqui, em um servidor X. Os não confiáveis ficam isolados das suas outras janelas e do teclado; os confiáveis não, então use só com servidores de confiança.

# Some fields need a look.
teaa35f1a = Alguns campos precisam de atenção.

# Delete {title} for everybody with access to {collection}?
t29fd2554 = Excluir { $title } para todos com acesso a { $collection }?

# Delete {title} from hosts.toml?
t596a83da = Excluir { $title } de hosts.toml?

# Duplicate
tfc330336 = Duplicar

# Save and connect
t83731197 = Salvar e conectar

# Server
t6d7302f2 = Servidor

# Database
tf09430d8 = Banco de dados

# Web
t72645431 = Web

# Container
tb23d4718 = Contêiner

# Firewall
tce907b11 = Firewall

# Router
t922ed124 = Roteador

# Storage
t34eddf06 = Armazenamento

# Home network
t674f73a8 = Rede doméstica

# Git
tb6b5540b = Git

# Compute
t5bc0161a = Computação

# Mail
t53707c4e = E-mail

# Desktop
t64215c53 = Desktop

# Bastion
tad4e4b8f = Bastion

# Test
t2ffcbe05 = Teste

# Command palette
td54729b1 = Paleta de comandos

# New tab
td3a5555a = Nova aba

# Close the pane
tc4b0686e = Fechar o painel

# Split right
t7c661b21 = Dividir à direita

# Split down
t8ebf2909 = Dividir abaixo

# Pane to the left
tbf1cdf5c = Painel à esquerda

# Pane to the right
tddc060a9 = Painel à direita

# Pane above
t842141d6 = Painel acima

# Pane below
t46005f86 = Painel abaixo

# Next tab
tb7610e87 = Próxima aba

# Previous tab
t2908a747 = Aba anterior

# Go to tab 1–9
t412f3292 = Ir para a aba 1–9

# Every tab, by host
td3226d58 = Todas as abas, por servidor

# Host list
t2c78c1d9 = Lista de servidores

# Find in the pane
t9dcfe91c = Localizar no painel

# Type in every pane
t5938c30b = Digitar em todos os painéis

# Settings
t4b058728 = Configurações

# Next theme
t9f747e2d = Próximo tema

# Previous theme
t4c4139ed = Tema anterior

# Quit
tc7b3d356 = Sair

# Paste
tb34a0288 = Colar

# Larger text
t0acf4d25 = Texto maior

# Smaller text
t3c9ae792 = Texto menor

# Text at its set size
tfd1aa132 = Texto no tamanho definido

# Scroll up a page
t3904cd21 = Rolar uma página para cima

# Scroll down a page
t6cd6ecda = Rolar uma página para baixo

# Back to the bottom
t8f497cc3 = Voltar ao final

# {key} alone is for typing: hold Cmd, Ctrl or Option with it.
ta81ce4ca = { $key } sozinha serve para digitar: segure Cmd, Ctrl ou Option junto.

# {key} alone is for typing: hold Ctrl or Alt with it.
t834f7024 = { $key } sozinha serve para digitar: segure Ctrl ou Alt junto.

# {key} belongs to the system: it never reaches Shellwing.
tcf75feb6 = { $key } pertence ao sistema: nunca chega ao Shellwing.

# {key} is {what}, which cannot be changed.
t01906ac3 = { $key } é “{ $what }”, que não pode ser alterado.

# config.toml, [shortcuts]: no shortcut is named `{name}`.
t331b3467 = config.toml, [shortcuts]: nenhum atalho se chama `{ $name }`.

# config.toml, [shortcuts] {name}: `{key}`: {error}
tb915b010 = config.toml, [shortcuts] { $name }: `{ $key }`: { $error }

# config.toml, [shortcuts] {name}: {why}
t71adda1f = config.toml, [shortcuts] { $name }: { $why }

# config.toml, [shortcuts]: `{key}` is given to both {kept} and {lost}; {kept} keeps it.
te833da37 = config.toml, [shortcuts]: `{ $key }` foi atribuído a { $kept } e a { $lost }; { $kept } fica com ele.

# config.toml, [shortcuts] {name}: `{key}` does not fit; only go_to_tab takes `digit` (1 to 9), and it takes nothing else.
ta357f31b = config.toml, [shortcuts] { $name }: `{ $key }` não serve; só go_to_tab aceita `digit` (de 1 a 9), e não aceita mais nada.

# Import
t573e0af4 = Importar

# No hosts in it.
tbb05fee9 = Não contém servidores.

# Looking for other SSH apps…
tb71f4415 = Procurando outros apps SSH…

# No other SSH app with hosts was found here.
tecba4271 = Nenhum outro app SSH com servidores foi encontrado aqui.

# Import hosts
t956d1e67 = Importar servidores

# Found on this computer
td811df9d = Encontrado neste computador

# Or a file another app exported
t9e7f0266 = Ou um arquivo exportado por outro app

# Choose a file or folder…
t379a2c2c = Escolher um arquivo ou uma pasta…

# PuTTY or KiTTY .reg, MobaXterm .mxtsessions, SecureCRT's XML or Sessions folder, Xshell .xts or Sessions folder, Tabby's config.yaml, an ssh config, or CSV with a header.
tf846074b = .reg do PuTTY ou KiTTY, .mxtsessions do MobaXterm, XML ou pasta Sessions do SecureCRT, .xts ou pasta Sessions do Xshell, config.yaml do Tabby, uma configuração ssh ou CSV com cabeçalho.

# Passwords are not imported: a host asks for its own the first time, and offers to keep it in the vault.
t1b7a0685 = As senhas não são importadas: cada servidor pede a sua na primeira vez e oferece guardá-la no cofre.

# Import from {app}
tb591e273 = Importar do { $app }

# {n} host checked, of {total}. | {n} hosts checked, of {total}.
t18f6360c =
    { $n ->
        [one] { $n } servidor marcado de { $total }.
       *[other] { $n } servidores marcados de { $total }.
    }

# {was} here is another host: named apart.
t3dd0ab4e = Já existe aqui outro servidor chamado { $was }: recebe outro nome.

# Already here as {name}.
t536acaec = Já está aqui como { $name }.

# Added: other hosts go through it.
te8d2ba16 = Adicionado: outros servidores passam por ele.

# No group
t2721e1c1 = Sem grupo

# {n} left out | {n} left out
t1e072c4b =
    { $n ->
        [one] { $n } deixado de fora
       *[other] { $n } deixados de fora
    }

# This is not a file hosts can be imported from.
tad56c9e9 = Não é possível importar servidores deste arquivo.

# {app} keeps this file encrypted, so it cannot be read here.
t63a77bb5 = O { $app } mantém este arquivo criptografado, por isso ele não pode ser lido aqui.

# Cannot read it: {e}
t4fd94e68 = Não é possível ler: { $e }

# Its key is PuTTY's .ppk: convert it with puttygen (OpenSSH format) to sign in with it.
t484769a7 = A chave é um .ppk do PuTTY: converta-a com o puttygen (formato OpenSSH) para entrar com ela.

# Its key {name} is in {app}'s own store: export it there, then add it under Keys.
t09682d64 = A chave { $name } está no armazenamento próprio do { $app }: exporte-a lá e depois adicione-a em Chaves.

# Its jump host {name} is not among the hosts: left out.
t00d9e5b4 = O servidor de salto { $name } não está entre os servidores: deixado de fora.

# {name}: not SSH ({protocol})
te0c47344 = { $name }: não é SSH ({ $protocol })

# {name}: no address
t9f701d25 = { $name }: sem endereço

# Reading…
t6aad8485 = Lendo…

# Back
tc2954bc2 = Voltar

# Import {n} host | Import {n} hosts
t040b2da6 =
    { $n ->
        [one] Importar { $n } servidor
       *[other] Importar { $n } servidores
    }

# Not read: the password was not given.
tf6b727a9 = Não lido: a senha não foi informada.

# Not read: the system did not allow it.
t0def7028 = Não lido: o sistema não permitiu.

# This system cannot ask for an administrator's password.
t8f851e9c = Este sistema não pode pedir a senha de um administrador.

# Not read: {why}
t9133669c = Não lido: { $why }

# Ed25519 (recommended)
t7294b934 = Ed25519 (recomendado)

# A name, to tell your keys apart
t6c2bbece = Um nome, para distinguir suas chaves

# Copied the public key.
t66ef7116 = Chave pública copiada.

# Not renamed: {e}
t836adf69 = Não renomeada: { $e }

# Servers that have its public key will not let it in from here any more, from any of your devices.
tf9cc53bc = Os servidores que têm a chave pública dela deixarão de aceitá-la a partir daqui, em qualquer um dos seus dispositivos.

# {hosts} sign in with it: they will try the other ways (the agent, ~/.ssh, a password) instead. Servers that have its public key keep it until you remove it there.
t20cb489a = { $hosts } entram com ela: passarão a tentar as outras formas (o agente, ~/.ssh, uma senha). Os servidores que têm a chave pública dela a mantêm até você removê-la lá.

# Deleted {name}.
t0c7abd07 = Excluída: { $name }.

# Not deleted: {e}
t12e9b880 = Não excluída: { $e }

# The two passphrases differ.
teb0402b1 = As duas frases secretas são diferentes.

# the key is not in the vault
t7ab0b044 = a chave não está no cofre

# the key does not read
t25447ed3 = a chave não pode ser lida

# Written to {path} and its .pub.
td30d7d74 = Gravada em { $path } e no seu .pub.

# Not written: {e}
t3ccd278a = Não gravada: { $e }

# Cannot read {path}: {e}
t7b24cd24 = Não é possível ler { $path }: { $e }

# Saved the passphrase of {file_name}: it is not asked for again.
t181d393f = Frase secreta de { $file_name } salva: ela não será pedida de novo.

# Not saved: {e}
ta9354472 = Não salva: { $e }

# {name} is in the vault now, and syncs to your other devices.
td3d8b162 = { $name } agora está no cofre e é sincronizada com seus outros dispositivos.

# {name} is in the ssh-agent now, until it stops.
tf6d081d4 = { $name } agora está no ssh-agent, até que ele pare.

# Not added: {e}
t6ff07db9 = Não adicionada: { $e }

# {name} is out of the ssh-agent.
t5ed55b5e = { $name } foi removida do ssh-agent.

# Not removed: {e}
t7b4feb23 = Não removida: { $e }

# The key is not in the vault.
t16ca2ffe = A chave não está no cofre.

# Forgot the passphrase of {path}: it is asked for again.
t4b242af5 = Frase secreta de { $path } esquecida: ela será pedida de novo.

# Not forgotten: {e}
t0e3fb3a1 = Não esquecida: { $e }

# not used by a host yet
t7a67a5ad = ainda não usada por nenhum servidor

# used by {one}
tefe52e8f = usada por { $one }

# used by {a}, {b}
t72e17f0a = usada por { $a }, { $b }

# used by {n} host | used by {n} hosts
tb629858a =
    { $n ->
        [one] usada por { $n } servidor
       *[other] usada por { $n } servidores
    }

# Copy public key
tccf653ee = Copiar chave pública

# Install on…
t0dfc84df = Instalar em…

# no passphrase
ta9025922 = sem frase secreta

# passphrase saved in the vault
t2c083e8e = frase secreta salva no cofre

# passphrase
t643fc8a5 = frase secreta

# also in the vault
t867e5d66 = também no cofre

# the same key as {path}
t0e1c7d80 = a mesma chave que { $path }

# the same key as {name} in the vault
tdfa4ba60 = a mesma chave que { $name } no cofre

# only in the agent
t3c2bc198 = somente no agente

# the agent's key
t7e7cc13c = a chave do agente

# (no comment)
t786863d8 = (sem comentário)

# Export to file…
t7b929ace = Exportar para arquivo…

# A passphrase, or none
t79f95085 = Uma frase secreta, ou nenhuma

# The passphrase again
tc3b3d2cc = A frase secreta de novo

# Remove from ssh-agent
td745f8e9 = Remover do ssh-agent

# Add to ssh-agent
t44341cdb = Adicionar ao ssh-agent

# Copy to vault
t7ac5f049 = Copiar para o cofre

# Save its passphrase…
tf91e6d9a = Salvar a frase secreta…

# Forget its saved passphrase
tc70cfa3b = Esquecer a frase secreta salva

# Generate a key
t1824e4f6 = Gerar uma chave

# Only for you, to tell your keys apart.
te5c40fac = Só para você, para distinguir suas chaves.

# Type
td155d06d = Tipo

# Ed25519 is short, fast and taken by OpenSSH 6.5 and later. RSA for very old servers.
t4146388b = Ed25519 é curta, rápida e aceita pelo OpenSSH 6.5 e posteriores. RSA para servidores muito antigos.

# Comment
t984f9ffe = Comentário

# Written at the end of the public key: what a server's authorized_keys shows.
t2a1f71fd = Escrito no final da chave pública: o que o authorized_keys de um servidor mostra.

# Kept in the vault, encrypted, and synced to your other devices, so it needs no passphrase. To use it with ssh or git too, export it to a file, with a passphrase if you like.
t504dc243 = Guardada no cofre, criptografada e sincronizada com seus outros dispositivos, por isso não precisa de frase secreta. Para usá-la também com ssh ou git, exporte-a para um arquivo, com uma frase secreta se quiser.

# Generating…
t12235a6b = Gerando…

# Generate
tc75562b2 = Gerar

# {name} is ready
t17060a6b = { $name } está pronta

# A server lets this key in once its public key is in the server's ~/.ssh/authorized_keys. Install it on a host from here, or copy it into a web console (GitHub, GitLab, a cloud provider).
tb989d6aa = Um servidor aceita esta chave quando a chave pública dela está em ~/.ssh/authorized_keys do servidor. Instale-a em um servidor daqui ou copie-a para um console web (GitHub, GitLab, um provedor de nuvem).

# Public key
t9981f45f = Chave pública

# Done
t8dd31791 = Concluído

# Install on a host…
ta8313af0 = Instalar em um servidor…

# Import a key file as administrator
t74d1b96b = Importar um arquivo de chave como administrador

# Rename the key
t2c552c07 = Renomear a chave

# Rename
tc80631f3 = Renomear

# Export {name}
t78124df8 = Exportar { $name }

# Written as an OpenSSH key file, for ssh, git and other tools, with its .pub beside it. A passphrase locks the file; the key in the vault stays as it is.
teb4d5edb = Gravada como um arquivo de chave OpenSSH, para ssh, git e outras ferramentas, com o .pub ao lado. Uma frase secreta protege o arquivo; a chave no cofre continua como está.

# Passphrase
t191a2dc5 = Frase secreta

# Again
t58fb56ef = Novamente

# Choose where…
t65990425 = Escolher onde…

# Unlock {path}
ta8940d72 = Desbloquear { $path }

# This key file has a passphrase. Type it to copy the key into the vault, where it needs none; the file stays as it is.
tc1aba63d = Este arquivo de chave tem uma frase secreta. Digite-a para copiar a chave para o cofre, onde ela não precisa de uma; o arquivo continua como está.

# This key file has a passphrase. Type it to hand the key to the ssh-agent; the file stays as it is.
td51093f3 = Este arquivo de chave tem uma frase secreta. Digite-a para entregar a chave ao ssh-agent; o arquivo continua como está.

# Save the passphrase of {path}
ta6e38b23 = Salvar a frase secreta de { $path }

# Kept in the vault, encrypted, and synced: shellwing then signs in with this key without asking, here and on your other devices.
tf45d84f1 = Guardada no cofre, criptografada e sincronizada: o shellwing passa a entrar com esta chave sem perguntar, aqui e nos seus outros dispositivos.

# Asking the agent…
tabb2eee4 = Consultando o agente…

# No agent, or it holds no keys.
tcad7e01e = Nenhum agente, ou ele não tem chaves.

# Keys
t602b32ed = Chaves

# Import key file…
t68acce01 = Importar arquivo de chave…

# As administrator…
t51b1ff28 = Como administrador…

# Generate key…
t0d000853 = Gerar chave…

# Keys for signing in to servers. Keys you generate or import here are kept in the vault, encrypted, and sync to your other devices.
t9da1da72 = Chaves para entrar em servidores. As chaves que você gera ou importa aqui são guardadas no cofre, criptografadas, e sincronizadas com seus outros dispositivos.

# The vault cannot be read: {p}
t650c9448 = Não é possível ler o cofre: { $p }

# In the vault
ta13ebcfd = No cofre

# synced · used only by shellwing
t0714e99d = sincronizadas · usadas somente pelo shellwing

# None yet: generate one, or copy one of your key files here.
t95a0c0ba = Nenhuma ainda: gere uma ou copie um dos seus arquivos de chave para cá.

# In ~/.ssh
t6420c141 = Em ~/.ssh

# key files on this computer, shared with ssh and git
t77dbc96f = arquivos de chave neste computador, compartilhados com ssh e git

# No key files.
t4c7991a8 = Nenhum arquivo de chave.

# In ssh-agent
t98288f7c = No ssh-agent

# offered first when connecting
tc83ab408 = oferecidas primeiro ao conectar

# Hosts set to Automatic try the agent, then ~/.ssh, then the vault, then ask for a password.
t0d28103e = Servidores configurados como Automático tentam o agente, depois ~/.ssh, depois o cofre e então pedem uma senha.

# Find a host, or type one to find hashed lines
te7877ac6 = Encontre um servidor ou digite um para encontrar linhas com hash

# Cannot read known_hosts: {e}
t57b65f0a = Não é possível ler known_hosts: { $e }

# Removed line {line}: the next connection to it asks about its key. The file as it was is known_hosts.old.
tcea0bcc6 = Linha { $line } removida: a próxima conexão a ele perguntará sobre a chave. O arquivo anterior ficou em known_hosts.old.

# No hosts yet: each one you trust is added here.
t72b46d71 = Nenhum servidor ainda: cada um em que você confiar é adicionado aqui.

# None matches.
tc9b5b29c = Nenhum corresponde.

# a hashed name
t87a3980c = um nome com hash

# {typed} (hashed)
t63339ce3 = { $typed } (com hash)

# {names} and {hashed} hashed
ta6fa2480 = { $names } e { $hashed } com hash

# revoked
tb4e706f5 = revogada

# certificate authority
t5299ff65 = autoridade certificadora

# {fingerprint} · line {line}
t25339948 = { $fingerprint } · linha { $line }

# Known hosts
tc951355f = Servidores conhecidos

# Open known_hosts
t61ed7186 = Abrir known_hosts

# The servers you trust, and their keys, from {path}: the same file ssh reads. A server showing another key is refused, and its pane says what changed. Removing one makes the next connection ask about it again.
tfb280692 = Os servidores em que você confia, e suas chaves, de { $path }: o mesmo arquivo que o ssh lê. Um servidor que mostra outra chave é recusado, e o painel dele diz o que mudou. Remover um faz a próxima conexão perguntar sobre ele de novo.

# {n} key | {n} keys
tc219ff12 =
    { $n ->
        [one] { $n } chave
       *[other] { $n } chaves
    }

# Snippet library
t1212f75d = Biblioteca de trechos

# Its tool (PostgreSQL)
tff6e50ee = Sua ferramenta (PostgreSQL)

# Its task (Backup)
t301e1817 = Sua tarefa (Backup)

# Both (PostgreSQL · Backup)
t116d70a6 = Ambas (PostgreSQL · Backup)

# Find a command: a tool, a task, a word…
tc13a8368 = Encontre um comando: uma ferramenta, uma tarefa, uma palavra…

# The vault is locked: unlock it in the main window.
te327c98e = O cofre está bloqueado: desbloqueie-o na janela principal.

# {n} snippet added: run it from the palette. | {n} snippets added: run them from the palette.
t10cc86d5 =
    { $n ->
        [one] { $n } trecho adicionado: execute-o pela paleta.
       *[other] { $n } trechos adicionados: execute-os pela paleta.
    }

# Every task
t27417099 = Todas as tarefas

# Not added yet
t18e34a9e = Ainda não adicionados

# Every tool
t72ff8f76 = Todas as ferramentas

# Added
tb35c3185 = Adicionados

# Nothing here: try other words, or every task.
ta46c06a7 = Nada aqui: tente outras palavras ou todas as tarefas.

# Makes changes
te471188d = Faz alterações

# Added ✓
t27d1ed36 = Adicionado ✓

# Added ×{n}
t9e411fdf = Adicionado ×{ $n }

# Click a command to see it, and adjust it before adding. Tick those to add.
t72e679a1 = Clique em um comando para vê-lo e ajustá-lo antes de adicionar. Marque os que quiser adicionar.

# Docs ↗
tffd1857b = Documentação ↗

# Options
t353e0d45 = Opções

# Run with sudo
tf1d67ebb = Executar com sudo

# it needs root
t992a8383 = precisa de root

# Runs
t9a5f395b = Execução

# On the host
t1842b6cb = No servidor

# In a Docker container: asks which
te04a8531 = Em um contêiner Docker: pergunta qual

# The files it reads or writes are the container's.
t33082a7c = Os arquivos que ele lê ou grava são os do contêiner.

# None: it asks, or goes without
tfc99399b = Nenhum: ele pede a senha ou segue sem ela

# It reads {var}: give it an environment that has it.
t1a1c7252 = Ele lê { $var }: dê a ele um ambiente que a tenha.

# has {var}
t4d81af7a = tem { $var }

# None yet: add one in Settings › Environments.
t769ef9f6 = Nenhum ainda: adicione um em Configurações › Ambientes.

# Asked when it runs: {names}. Once added, it is yours: edit it in its window.
tfc64bf52 = Perguntado ao executar: { $names }. Depois de adicionado, ele é seu: edite-o na janela dele.

# It makes changes: added, it is typed and waits for you to press Enter.
t4d1e2e14 = Faz alterações: depois de adicionado, ele é digitado e espera você pressionar Enter.

# In your snippets once. | In your snippets {n} times.
t028f555a =
    { $n ->
        [one] { $n } vez nos seus trechos.
       *[other] { $n } vezes nos seus trechos.
    }

# {n} picked | {n} picked
tf375b5ad =
    { $n ->
        [one] { $n } selecionado
       *[other] { $n } selecionados
    }

# Tick the commands to add; each becomes a snippet of your own.
t71ce4a51 = Marque os comandos para adicionar; cada um vira um trecho seu.

# Add {n} to my snippets | Add {n} to my snippets
t9489b24d =
    { $n ->
        [one] Adicionar { $n } aos meus trechos
       *[other] Adicionar { $n } aos meus trechos
    }

# Add to my snippets
tacccc61d = Adicionar aos meus trechos

# File under
t7be2ad59 = Classificar em

# No config directory to save to.
ta5f37ee4 = Nenhum diretório de configuração para salvar.

# Theme `{want}` not found; using the default.
t1cda442a = Tema `{ $want }` não encontrado; usando o padrão.

# {host} sets its own theme in hosts.toml; change it there.
tec1b7d01 = { $host } define o próprio tema em hosts.toml; altere-o lá.

# Theme `{id}` not found.
tbae3cb18 = Tema `{ $id }` não encontrado.

# Blue and slate
t35054fdd = Azul e ardósia

# Purple and pink
t6bae17ce = Roxo e rosa

# Green and teal
tfc91f0a1 = Verde e azul-petróleo

# Warm
t04a11b58 = Quente

# Gradient
t993c73c5 = Gradiente

# Gradient, light
td685ec1b = Gradiente, claro

# Light, cool
tf5e42e16 = Claro, frio

# Light, warm
tdd571ace = Claro, quente

# Your themes
tb2e83d5c = Seus temas

# Image from {host}
t4b45629c = Imagem de { $host }

# Loading the image…
t457184d7 = Carregando a imagem…

# Image not synced yet
tf2f8e6ad = Imagem ainda não sincronizada

# The image cannot be shown
teac0ac18 = Não é possível mostrar a imagem

# The pasted image
t10124f90 = A imagem colada

# {name} is too large to be an image here.
t6bd6209b = { $name } é grande demais para ser uma imagem aqui.

# {name} cannot be read: {e}
teff3eb8d = Não é possível ler { $name }: { $e }

# {name} is not an image that can be read.
t53d0e31f = { $name } não é uma imagem que possa ser lida.

# {name} is too large an image ({width} × {height}).
tad4a609d = { $name } é uma imagem grande demais ({ $width } × { $height }).

# {host} is on a private network: its image was not downloaded.
tf2d8cf64 = { $host } está numa rede privada: a imagem não foi baixada.

# {name} could not be downloaded: {e}
t66ad539b = Não foi possível baixar { $name }: { $e }

# New note
t788c7123 = Nova nota

# Title
t24d471a9 = Título

# No category
t44662d06 = Sem categoria

# Write here; Markdown shows as you type.
t117b6c23 = Escreva aqui; o Markdown aparece enquanto você digita.

# Copy Image
tf77eb889 = Copiar imagem

# Save Image As…
t4504c6b5 = Salvar imagem como…

# It is not in the vault any more.
td11af590 = Ele não está mais no cofre.

# Images show in Markdown notes; this note is plain text.
tb31ba949 = As imagens aparecem em notas Markdown; esta nota é texto simples.

# Images go in while the note is edited.
t6f2c9191 = As imagens entram enquanto a nota é editada.

# shellwing downloads them from {hosts}, which learn your address. They are then kept in the vault, encrypted, and the links in the note are replaced.
t4f35c5e2 = O shellwing as baixa de { $hosts }, que ficam sabendo o seu endereço. Depois elas ficam no cofre, criptografadas, e os links da nota são substituídos.

# Some come over plain http: they could be altered on the way.
tfe0055bd = Algumas vêm por http simples: podem ser alteradas no caminho.

# Download the note's image from the web? | Download the note's {n} images from the web?
t81c8cf06 =
    { $n ->
        [one] Baixar da web a imagem da nota?
       *[other] Baixar da web as { $n } imagens da nota?
    }

# Export
te86349d3 = Exportar

# Export the note?
t3fc8daf1 = Exportar a nota?

# The note and its images are written as files, not encrypted: anyone who can read the folder can read them.
t599b6331 = A nota e suas imagens são gravadas como arquivos, sem criptografia: quem puder ler a pasta poderá lê-las.

# Exported without an image not synced yet. | Exported without {n} images not synced yet.
t9b7fcb65 =
    { $n ->
        [one] Exportada sem uma imagem ainda não sincronizada.
       *[other] Exportada sem { $n } imagens ainda não sincronizadas.
    }

# Not exported: {e}
t9d93dffa = Não exportada: { $e }

# Insert
t0f317948 = Inserir

# this note
t4545c373 = esta nota

# It goes from the vault, and from your other devices once they sync.
t3c6666da = Ele sai do cofre e dos seus outros dispositivos assim que eles sincronizarem.

# The vault's passphrase
tb66334ea = Frase secreta do cofre

# Wrong passphrase.
t2ebec270 = Frase secreta incorreta.

# For a host ▾
tfa622581 = Para um servidor ▾

# No hosts yet.
t71734b03 = Nenhum servidor ainda.

# Saving…
tbd79b37d = Salvando…

# Saved
t8a6979e4 = Salvas

# Plain text
tb7fc0ade = Texto simples

# Edit
tc2c76cb1 = Editar

# Preview
tfdda4cf7 = Prévia

# Sensitive
tf603feef = Confidencial

# Download Image | Download {n} Images
t2594894c =
    { $n ->
        [one] Baixar imagem
       *[other] Baixar { $n } imagens
    }

# Export…
tc8865b2d = Exportar…

# Sensitive: its text shows once it is you.
tb81fab1b = Confidencial: o texto aparece depois de confirmar que é você.

# Show the text
t0ff690fe = Mostrar o texto

# Confirm it is you in the system's prompt…
t3ab56316 = Confirme que é você na janela do sistema…

# No passphrase guards the vault, so anyone at this device can show it.
t41e36c52 = Nenhuma frase secreta protege o cofre, então qualquer pessoa neste dispositivo pode mostrá-lo.

# Insert into {pane}
t3d5eaffc = Inserir em { $pane }

# No shell to insert into
t825e7370 = Nenhum shell para inserir

# Insert Image…
t012e861b = Inserir imagem…

# Cut
t58e25863 = Recortar

# Copy as Markdown
t944799cd = Copiar como Markdown

# Turn into
t10726570 = Transformar em

# Text
t3e142d5e = Texto

# Heading 1
ta590c41a = Título 1

# Heading 2
ta490c287 = Título 2

# Heading 3
ta390c0f4 = Título 3

# Bulleted list
t1721be78 = Lista com marcadores

# Numbered list
t910847bb = Lista numerada

# To-do
t082e6fd6 = Tarefa

# Quote
t5abe5677 = Citação

# Callout
t10591ec3 = Destaque

# Code block
t3026e515 = Bloco de código

# Insert row above
t2f1931cd = Inserir linha acima

# Insert row below
t870e8f41 = Inserir linha abaixo

# Duplicate row
te898fcce = Duplicar linha

# Insert column left
tb49d3fc3 = Inserir coluna à esquerda

# Insert column right
t60f12dcc = Inserir coluna à direita

# Align left
tbdd22217 = Alinhar à esquerda

# Align center
t76127b0f = Centralizar

# Align right
t1a76ce88 = Alinhar à direita

# Grid style
t531d1674 = Estilo grade

# Striped style
t10b52f0d = Estilo listrado

# Header style
te7eb6c45 = Estilo com cabeçalho

# Minimal style
t70f1e7d5 = Estilo mínimo

# Delete row
t33de1b52 = Excluir linha

# Delete column
tea682d34 = Excluir coluna

# Delete table
t29ccb392 = Excluir tabela

# Untitled note
t4ce277c4 = Nota sem título

# Recent
t50e888fe = Recentes

# Snippets
t648fcbf7 = Trechos

# Environments
t273b1ccb = Ambientes

# Config files
t16261310 = Arquivos de configuração

# Notes
t0ff01b6a = Notas

# Actions
te65fb504 = Ações

# Find a host, or type user@host
t18f005c7 = Encontre um servidor ou digite user@host

# Find a saved password or passphrase
t8452d3bc = Encontre uma senha ou frase secreta salva

# Find a theme
ta5e61028 = Encontre um tema

# Connect
tdb89a899 = Conectar

# Connect to {typed}
t51c5c888 = Conectar a { $typed }

# a new address; a Host block in ~/.ssh/config applies
t69b58e6c = um endereço novo; um bloco Host em ~/.ssh/config se aplica

# {n} more in the snippet library… | {n} more in the snippet library…
t51d15edf =
    { $n ->
        [one] mais { $n } na biblioteca de trechos…
       *[other] mais { $n } na biblioteca de trechos…
    }

# Enter again to forget
t850e9777 = Enter de novo para esquecer

# Nothing matches.
tf8e003c3 = Nada corresponde.

# ↑↓ move   Tab next group   Enter go to   {new_tab} new tab   Esc close   type > for actions only
tb494b680 = ↑↓ mover   Tab próximo grupo   Enter ir   { $new_tab } nova aba   Esc fechar   digite > para ver só ações

# ↑↓ preview   Enter keep   Esc go back
t60259b3c = ↑↓ visualizar   Enter manter   Esc voltar

# ↑↓ move   Enter twice forget   Esc close   secrets are never shown
tbc202ae0 = ↑↓ mover   Enter duas vezes esquecer   Esc fechar   segredos nunca são mostrados

# Enter copies   Esc close
t39061ccf = Enter copia   Esc fechar

# Enter answers   Esc cancels
tbfdc091c = Enter responde   Esc cancela

# ↑↓ move   Enter choose   Esc cancel
te39c67c2 = ↑↓ mover   Enter escolher   Esc cancelar

# Find hosts
t1f19b271 = Encontre servidores

# sudo password
t6f1147fd = senha do sudo

# Run on hosts
t20ee87e4 = Executar em servidores

# Choose at least one host.
t98a361a0 = Escolha pelo menos um servidor.

# Type a command or a script first.
tc08b2b46 = Digite um comando ou um script primeiro.

# The script is larger than {kb} KB.
t3c583c57 = O script tem mais de { $kb } KB.

# none chosen
t23b63bc7 = nenhum escolhido

# {picked} of {all}
tb6c0f13d = { $picked } de { $all }

# No hosts yet: add one in the host list.
tb1cb1380 = Nenhum servidor ainda: adicione um na lista de servidores.

# chosen
t88efc3c3 = escolhido

# from #!
te59619bf = do #!

# default
t933b5bde = padrão

# Command or script
t9263b49e = Comando ou script

# Open file…
teb1476db = Abrir arquivo…

# From a snippet…
t5b2e9f81 = De um trecho…

# From the first line
t4eea1d86 = Da primeira linha

# No snippets yet.
ta1f34f58 = Nenhum trecho ainda.

# At most
tc4220f87 = No máximo

# at once
t73078e59 = ao mesmo tempo

# Give up after
t72dfc173 = Abandonar após

# seconds
t66b6cdea = segundos

# One host first, the rest when you say
t2dc320ca = Um servidor primeiro, o resto quando você disser

# Cmd+Enter runs
tc5c50a00 = Cmd+Enter executa

# Ctrl+Enter runs
t1e0dc7f1 = Ctrl+Enter executa

# Run
t8d57e66a = Executar

# Run on {n} host | Run on {n} hosts
t9f3562b2 =
    { $n ->
        [one] Executar em { $n } servidor
       *[other] Executar em { $n } servidores
    }

# Run it on {first} first, then the other {n}?
tdeeed76d = Executar primeiro em { $first } e depois nos outros { $n }?

# Run it on {n} host? | Run it on {n} hosts?
t15434ee6 =
    { $n ->
        [one] Executar em { $n } servidor?
       *[other] Executar em { $n } servidores?
    }

# {interpreter}, {n} line | {interpreter}, {n} lines
t8077e5da =
    { $n ->
        [one] { $interpreter }, { $n } linha
       *[other] { $interpreter }, { $n } linhas
    }

# signing in to {n} of {all}…
t6d7d8e5a = fazendo login em { $n } de { $all }…

# {n} of {all} signed in
t11ab0d58 = login feito em { $n } de { $all }

# at most {n} at once
td03e9105 = no máximo { $n } ao mesmo tempo

# Signing in: asked now, once
t41418846 = Login: pedido agora, uma vez

# {target}'s password, again: that one was refused
t39e81801 = Senha de { $target }, de novo: aquela foi recusada

# {target}'s password
t8989af6d = Senha de { $target }

# The passphrase of {key}
tb9a076ee = A frase secreta de { $key }

# A host key never seen before: {algorithm} {fingerprint}
t064ebede = Uma chave de servidor nunca vista antes: { $algorithm } { $fingerprint }

# Leave it out
taaff8ec9 = Deixar de fora

# Trust it
tf80adc72 = Confiar

# OK
t85e4b82f = OK

# {host} is left out: {why}
ta0fb8060 = { $host } fica de fora: { $why }

# sudo password, for every host
tf9f4ad8f = senha do sudo, para todos os servidores

# The same on every host
t09beea56 = A mesma em todos os servidores

# The script uses sudo: the password goes to each host with it, for sudo there to read, never on a command line, in a file or in what is shown. It is forgotten once the run starts.
t893beb5f = O script usa sudo: a senha vai para cada servidor junto com ele, para o sudo de lá ler, nunca em uma linha de comando, em um arquivo ou no que é mostrado. Ela é esquecida assim que a execução começa.

# Run on {host}
t53629f2b = Executar em { $host }

# started {when}
t0addf546 = iniciado às { $when }

# ran at {when}
tc1949fbd = executado às { $when }

# Again on the {n} that failed | Again on the {n} that failed
t40c7552d =
    { $n ->
        [one] De novo no { $n } que falhou
       *[other] De novo nos { $n } que falharam
    }

# Run again
t7e879b86 = Executar de novo

# {n} ok
t230daddb = { $n } ok

# {n} failed
t747c50a8 = { $n } com falha

# {n} to go
t9b6bf176 = { $n } a terminar

# {n} stopped
td3275eca = { $n } parados

# By output
t58a5f0b1 = Por saída

# By host
ta32dd06e = Por servidor

# The first host is done. The other {n} wait for you.
tdc76953e = O primeiro servidor terminou. Os outros { $n } esperam por você.

# Run on the other {n} | Run on the other {n}
t4cc1f343 =
    { $n ->
        [one] Executar no { $n } restante
       *[other] Executar nos outros { $n }
    }

# Still going: {hosts}
tfacf50d8 = Ainda em execução: { $hosts }

# Stopped before they ended: {hosts}
tde1adb0a = Parados antes de terminar: { $hosts }

# It printed more than {mb} MB: the rest is not kept.
t3b5e54ea = Imprimiu mais de { $mb } MB: o resto não é guardado.

# Open a tab
t85019429 = Abrir uma aba

# Given up on after {s} seconds.
tf245d128 = Abandonado após { $s } segundos.

# sudo did not take the password
tb2c3fb01 = o sudo não aceitou a senha

# sudo asked for a password
t31a388e9 = o sudo pediu uma senha

# did not connect
t7efff279 = não conectou

# given up on
t4eec5298 = abandonado

# signing in
t727e0a33 = fazendo login

# asks something
tf96d1b57 = pergunta algo

# signed in
t1a42c214 = login feito

# left out
t73aeedd4 = de fora

# waiting
teeda06a8 = aguardando

# sudo refused
tdd8b1f54 = recusado pelo sudo

# What each host says shows here, grouped by output.
taede9a2c = O que cada servidor diz aparece aqui, agrupado por saída.

# It runs as ssh host 'command' would, with no terminal, read whole before it starts, so nothing in it reads the rest of it. sudo's password is asked once, before it runs; anything else that asks (a y/n) gets no answer.
t82319ee2 = Ele roda como ssh host 'command' rodaria, sem terminal, lido por inteiro antes de começar, então nada nele lê o resto dele. A senha do sudo é pedida uma vez, antes de rodar; qualquer outra coisa que pergunte (um y/n) fica sem resposta.

# About
t58e096de = Sobre

# © {year} HivePaaS. The shellwing name and logo are trademarks of HivePaaS.
ta956e96f = © { $year } HivePaaS. O nome e o logotipo shellwing são marcas da HivePaaS.

# A native SSH client for Linux, macOS and Windows.
t182e3bec = Um cliente SSH nativo para Linux, macOS e Windows.

# Version {version} · {os} {arch}
t6bead5ae = Versão { $version } · { $os } { $arch }

# Copy version info
t3d37483c = Copiar informações da versão

# Copied: {info}
t183c7f9f = Copiado: { $info }

# This build has no release keys, so it cannot tell of updates.
t378182cb = Esta compilação não tem chaves de publicação, então não pode avisar sobre atualizações.

# shellwing {version} is ready
t3f4cfe3f = shellwing { $version } está pronto

# It is installed when you quit, or now with Restart.
t6fecac86 = Ele é instalado quando você sai, ou agora com Reiniciar.

# Restart now
t54001a7c = Reiniciar agora

# shellwing {version}
tfc977e10 = shellwing { $version }

# Looking for a newer version…
tf1905fe9 = Procurando uma versão mais recente…

# shellwing {version} is out
t5b33836c = shellwing { $version } está disponível

# Run these in a terminal. They check the download against the signed list of releases, and sudo asks for your password.
t98b9753c = Execute estes comandos em um terminal. Eles verificam o download com a lista assinada de versões, e o sudo pede a sua senha.

# Copied.
t53842cb1 = Copiado.

# Open in a local terminal
t556d080c = Abrir em um terminal local

# Skip this version
t2c33d756 = Pular esta versão

# How to update
t3603428f = Como atualizar

# Download and install
t1a99173f = Baixar e instalar

# {by} updates it: {command}
tdf2cab9c = Atualize usando { $by }: { $command }

# {by} updates it.
t84b27295 = Atualize usando { $by }.

# It is downloaded and checked, then installed when you quit.
tb46c0678 = Ele é baixado e verificado, e instalado quando você sai.

# Download the package for your system, and install it with {tool}.
tb4036d6e = Baixe o pacote para o seu sistema e instale-o com { $tool }.

# Download it, and install it in place of this one.
t39312eae = Baixe e instale no lugar desta versão.

# The download failed: {why}
t5e96c075 = O download falhou: { $why }

# The last look failed: {why}
td122a092 = A última verificação falhou: { $why }

# Up to date (looked {when}).
t4665ef60 = Atualizado (verificado { $when }).

# Not looked yet.
t5a3ba058 = Ainda não verificado.

# Updates
t483075a5 = Atualizações

# This is a beta: you are told of newer betas, and of the release that follows them.
t1d6a96bc = Esta é uma beta: você recebe avisos de betas mais recentes e da versão estável que vem depois delas.

# This is a release: you are told of newer releases, not of betas.
t9d2d8250 = Esta é uma versão estável: você recebe avisos de versões estáveis mais recentes, não de betas.

# Check now
t4f0e0677 = Verificar agora

# Look for updates once a day
tcc27ace1 = Procurar atualizações uma vez por dia

# It reads the list of releases from GitHub, and believes it only when its signatures verify. Nothing is downloaded or installed without you.
t258cdfd9 = A lista de versões é lida do GitHub e só é aceita quando suas assinaturas são válidas. Nada é baixado ou instalado sem você.

# Visit shellwing.app
t1d2ed540 = Visitar shellwing.app

# Report a problem
t7b1728e7 = Relatar um problema

# Release notes
tc52ab62f = Notas da versão

# Open source
ta04619ce = Código aberto

# shellwing is free and open source, under the GPL-3.0-or-later license. Read the code, build it yourself, report a problem or send a fix.
tcde8081b = O shellwing é software livre e de código aberto, sob a licença GPL-3.0-or-later. Leia o código, compile você mesmo, relate um problema ou envie uma correção.

# Open source · Apache-2.0
tf33a768c = Código aberto · Apache-2.0

# A self-hosted platform for your apps, built on Docker Swarm
t92f3d8ce = Uma plataforma auto-hospedada para seus apps, baseada em Docker Swarm

# On your servers
te5da903f = Nos seus servidores

# Deploy from Git or a Docker image, with domains and automatic TLS, logs, metrics and scaling across a cluster.
t1671a837 = Faça deploy a partir do Git ou de uma imagem Docker, com domínios e TLS automático, logs, métricas e escala em um cluster.

# On your own computer
t75086b1c = No seu próprio computador

# Install it on your own computer in place of Docker Desktop or Portainer, and still get full app management on top.
te7416212 = Instale no seu computador no lugar do Docker Desktop ou do Portainer e ganhe ainda um gerenciamento completo de apps.

# 300+ one-click apps
t6a770b0a = Mais de 300 apps com um clique

# Deploy on push
tb0bfc3e9 = Deploy a cada push

# Pull request previews
t4e4180fc = Prévias de pull requests

# Automatic TLS
t9e9776ed = TLS automático

# Live logs and metrics
tebe3a172 = Logs e métricas ao vivo

# Functions
t69d796ee = Funções

# Visit hivepaas.com
t0632975a = Visitar hivepaas.com

# From the same author
te0bcddcb = Do mesmo autor

# You might also like HivePaaS, from the same author: an open-source, self-hosted alternative to Heroku, Render and Coolify.
t1f5d9e97 = Talvez você goste do HivePaaS, do mesmo autor: uma alternativa auto-hospedada e de código aberto ao Heroku, Render e Coolify.

# Press a digit from 1 to 9 with the keys to hold, such as {example}: the digit picks the tab.
t78f1fce3 = Pressione um dígito de 1 a 9 junto com as teclas a segurar, como { $example }: o dígito escolhe a aba.

# Reset
t0ac8a560 = Redefinir

# Every shortcut back to its own keys?
t0990da9d = Voltar cada atalho às suas teclas padrão?

# Reset all
t3c3d9373 = Redefinir tudo

# Keyboard
t27d1a714 = Teclado

# Window shortcuts never reach the remote host. Click a key to change it and press the new one (Esc stops); + gives a shortcut one more key, × takes one away. They are kept in config.toml, under [shortcuts].
t1bbd6fbd = Os atalhos da janela nunca chegam ao servidor remoto. Clique em uma tecla para alterá-la e pressione a nova (Esc cancela); + dá mais uma tecla a um atalho, × tira uma. Ficam guardados em config.toml, em [shortcuts].

# In a pane, cannot be changed
t8cb028d3 = Em um painel, não podem ser alteradas

# Use it anyway
t52386b1a = Usar mesmo assim

# Swap
te519d36e = Trocar

# {key} is {what}'s. Swap: {what} gets {old}.
t6f1711d7 = { $key } é de “{ $what }”. Trocar: “{ $what }” fica com { $old }.

# Move it here
t8c848ead = Mover para cá

# {key} is {what}'s. Move it here: {what} loses it.
t010b909d = { $key } é de “{ $what }”. Mover para cá: “{ $what }” perde { $key }.

# Programs in the terminal use {key} (as Ctrl+R searches the shell's history, Alt+B goes back a word): taken here, they never get it.
t13aa6dfa = Programas no terminal usam { $key } (Ctrl+R busca no histórico do shell, Alt+B volta uma palavra): se for usado aqui, eles nunca o recebem.

# Press keys…
t2b545ab4 = Pressione as teclas…

# General
t5561fd0b = Geral

# Appearance
t31018b9f = Aparência

# SSH and security
t699a801a = SSH e segurança

# Vault
td432914f = Cofre

# AI
t23d5135f = AI

# Its password (an app password, where it has them)
t22e462e2 = A senha dele (uma senha de app, onde houver)

# none: the top of the bucket
t863402f7 = nenhuma: a raiz do bucket

# Its secret
t5709e2db = O segredo da chave

# https://github.com/you/vault.git, or git@github.com:you/vault.git
t367f2cde = https://github.com/you/vault.git, ou git@github.com:you/vault.git

# its default
t7a9ad2ec = o padrão dele

# none: git signs in as it does for you
ta76cffea = nenhum: o git entra como faz para você

# any (Gitea: yours)
t97f35ca8 = qualquer um (Gitea: o seu)

# choose a folder
tcaf5dc69 = escolha uma pasta

# Fill in the server's address.
tce72be65 = Preencha o endereço do servidor.

# The address starts with https://.
t2ed84724 = O endereço começa com https://.

# Fill in the address, the user and the password.
t164fba4d = Preencha o endereço, o usuário e a senha.

# Fill in the endpoint, the bucket, the access key and its secret.
t03a28947 = Preencha o endpoint, o bucket, a chave de acesso e o segredo dela.

# The endpoint starts with https://.
t64b883b1 = O endpoint começa com https://.

# Fill in the repository's address.
t5a34e5a8 = Preencha o endereço do repositório.

# {shell} (yours)
tb016dc00 = { $shell } (o seu)

# e.g. code -n, or /path/to/editor --flag
t455dc834 = ex.: code -n ou /path/to/editor --flag

# Keep logs here
t18df5c77 = Guardar registros aqui

# Cannot sign in: {e}
tf98212fb = Não foi possível entrar: { $e }

# git {version} is installed: Connect again.
t92082827 = git { $version } está instalado: conecte de novo.

# When Apple's installer is done, Connect again.
t2a710b03 = Quando o instalador da Apple terminar, conecte de novo.

# git was not installed: {e}
t7a63a93e = o git não foi instalado: { $e }

# Default ({font})
ta4f51a30 = Padrão ({ $font })

# Default (the system's)
te8f5418b = Padrão (a do sistema)

# Font
ta77a5eb0 = Fonte

# App text
t3b9aa6d3 = Texto do app

# Tabs, the host list, the palette, panels and this page.
t494aba4f = Abas, a lista de servidores, a paleta, os painéis e esta página.

# Size
ta6478e7c = Tamanho

# The system's
t520575b7 = O do sistema

# Custom…
ta391dade = Personalizado…

# The file's path is added at the end. Enter saves.
tdd20b80e = O caminho do arquivo é adicionado ao final. Enter salva.

# As the system ({language})
t4956cc50 = Como o sistema ({ $language })

# Language
t9a73db9b = Idioma

# shellwing's own words: menus, Settings, questions and notices. What the terminal shows is the server's. This device's own: it does not sync.
t74b71048 = Os textos do próprio shellwing: menus, Configurações, perguntas e avisos. O que o terminal mostra vem do servidor. Próprio deste dispositivo: não é sincronizado.

# Text editor for remote files
tdc31a941 = Editor de texto para arquivos remotos

# Edit in text editor, in the Files panel, opens a copy with it and writes the file back each time you save. The system's is the desktop's app for plain text.
t3110b5e8 = Editar no editor de texto, no painel Arquivos, abre uma cópia com ele e grava o arquivo de volta sempre que você salva. O do sistema é o app de texto simples da área de trabalho.

# Open the last tabs at start
tf7087aa7 = Abrir as últimas abas ao iniciar

# When shellwing starts without a host, it opens the tabs and splits it had.
t2b27326b = Quando o shellwing inicia sem um servidor, ele abre as abas e divisões que tinha.

# Hosts from another app
tffafddac = Servidores de outro app

# PuTTY, KiTTY, MobaXterm, SecureCRT, Xshell, Tabby, ~/.ssh/config or a CSV file: their hosts, groups, jump hosts and forwards, shown first to choose from. Passwords are not imported.
tdd2c4842 = PuTTY, KiTTY, MobaXterm, SecureCRT, Xshell, Tabby, ~/.ssh/config ou um arquivo CSV: seus servidores, grupos, servidores de salto e redirecionamentos, mostrados primeiro para escolher. As senhas não são importadas.

# Import…
tecc08160 = Importar…

# These settings, for this device. Edits in the file apply at once.
ta0344de9 = Estas configurações, para este dispositivo. Edições no arquivo valem na hora.

# Hosts and groups. Synced. No secrets.
t4f70346f = Servidores e grupos. Sincronizado. Sem segredos.

# Your own themes.
t69ba4c4a = Seus próprios temas.

# Passwords and passphrases, encrypted.
te7f92b47 = Senhas e frases secretas, criptografadas.

# System keychain
t6e83d178 = Chaveiro do sistema

# The vault's unlock key.
tdda85be9 = A chave de desbloqueio do cofre.

# Where settings live
tf63ebef5 = Onde ficam as configurações

# Open folder
t713a842f = Abrir pasta

# System
t8f3184bc = Sistema

# Dark
t45570065 = Escuro

# Light
t8ad2f8cf = Claro

# Block
tbd074002 = Bloco

# Bar
tdbbc7dba = Barra

# Underline
t9a85011f = Sublinhado

# When scrolling
t3d08a81e = Ao rolar

# Terminal text
t77332e04 = Texto do terminal

# Line height
t64697aa2 = Altura da linha

# Cursor
t558a502f = Cursor

# Scrollbar
ta18bd919 = Barra de rolagem

# Padding at the sides
tb8eb2bd4 = Margem nas laterais

# Top and bottom
t56cbbc10 = Em cima e embaixo

# Mode
t534e7732 = Modo

# The system is light now.
t50316f27 = O sistema está claro agora.

# The system is dark now.
t03050ee1 = O sistema está escuro agora.

# App colors
tfb1bbe48 = Cores do app

# The app's own: the title bar, the host list, panels and menus. The terminal takes its theme's.
t04d02208 = As do próprio app: a barra de título, a lista de servidores, os painéis e os menus. O terminal usa as do seu tema.

# Terminal theme, light
t7d52a2a4 = Tema do terminal, claro

# Terminal theme, dark
tf112a1cc = Tema do terminal, escuro

# The terminal's, while the app is light; the dark mode keeps its own. A host can have its own in its editor.
t0b71dd99 = O do terminal, enquanto o app está claro; o modo escuro mantém o seu. Um servidor pode ter o próprio no editor dele.

# The terminal's, while the app is dark; the light mode keeps its own. A host can have its own in its editor.
te0303bcd = O do terminal, enquanto o app está escuro; o modo claro mantém o seu. Um servidor pode ter o próprio no editor dele.

# Open themes folder
t64678e0b = Abrir pasta de temas

# A .toml file there is a theme of your own.
t39d886ba = Um arquivo .toml lá é um tema seu.

# The app's accent from the theme
t44bdb829 = Cor de destaque do app a partir do tema

# The logo, what is chosen and the main buttons take the theme's accent color, in place of the app's blue. One too close to the app's background is not taken.
tb5bd008b = O logotipo, o que está selecionado e os botões principais usam a cor de destaque do tema, no lugar do azul do app. Uma cor próxima demais do fundo do app não é usada.

# {k} k
t450902df = { $k } mil

# Reconnect
tcf289b06 = Reconectar

# Close
tcd86acc3 = Fechar

# Scrollback
tc94c2be7 = Histórico de rolagem

# Lines kept per pane, now {lines}. More uses more memory; new tabs get it.
tc5513217 = Linhas mantidas por painel, agora { $lines }. Mais linhas usam mais memória; novas abas recebem o valor.

# When a connection drops
ted68d834 = Quando uma conexão cai

# Reconnect tries again by itself, waiting longer each time. After exit, Enter connects again.
t0b750ede = Reconectar tenta de novo sozinho, esperando mais a cada vez. Após o encerramento, Enter conecta de novo.

# Local shell
tef7cec6a = Shell local

# What a local terminal runs (This computer, in the host list), with its arguments. Empty: your own shell. Enter saves; new local tabs get it.
t235223ca = O que um terminal local executa (Este computador, na lista de servidores), com os argumentos. Vazio: o seu próprio shell. Enter salva; novas abas locais recebem o valor.

# Raw
ta689aa91 = Bruto

# Change…
t16f90b63 = Alterar…

# Use the default
tb4d20336 = Usar o padrão

# Session logs
t6a227ff6 = Registros de sessão

# Log every session
t31c190d6 = Registrar todas as sessões

# From its start, to a file of its own. A host can say otherwise in its editor, and Start Logging in a pane's menu logs it from then on.
t8375a4d9 = Desde o início, em um arquivo próprio. Um servidor pode definir outra coisa no editor dele, e Iniciar registro, no menu de um painel, registra a partir daí.

# Format
tffb0ff72 = Formato

# Text: the lines as they were left, colors gone; what full-screen programs (vim, htop) draw is left out. Raw: every byte as it came, which cat or less -R shows in color.
taebfec38 = Texto: as linhas como ficaram, sem cores; o que programas de tela cheia (vim, htop) desenham fica de fora. Bruto: cada byte como chegou, que cat ou less -R mostram em cores.

# The time on each line
t2324a143 = A hora em cada linha

# Each line of a text log starts with the date and time it was written.
taf5ab76c = Cada linha de um registro em texto começa com a data e a hora em que foi gravada.

# Folder
t06357afd = Pasta

# Logs go to {folder}. Only you can read them: they hold whatever the sessions showed.
t6395d795 = Os registros vão para { $folder }. Só você pode lê-los: eles contêm tudo o que as sessões mostraram.

# No folder for logs: choose one.
tc4affc27 = Nenhuma pasta para registros: escolha uma.

# Found. Its keys are tried first.
tc1d15743 = Encontrado. As chaves dele são tentadas primeiro.

# Not found (SSH_AUTH_SOCK is not set).
tb9e9f472 = Não encontrado (SSH_AUTH_SOCK não está definido).

# Refuse
t9bcada35 = Recusar

# Unknown host keys
t2534790f = Chaves de servidor desconhecidas

# Ask shows the fingerprint and sends nothing until you accept it. Accept new records it and goes on; Refuse turns it away. A changed key is always refused. A host can have its own in its editor.
t2abc37ff = Perguntar mostra a impressão digital e não envia nada até você aceitá-la. Aceitar novas a registra e continua; Recusar a rejeita. Uma chave alterada é sempre recusada. Um servidor pode ter a própria configuração no editor dele.

# A check every so often, so idle connections are not dropped and dead ones are noticed after 3 misses. Now {now}. A host's own (or its ServerAliveInterval) wins.
t1b853e51 = Uma verificação de tempos em tempos, para que conexões ociosas não caiam e as mortas sejam percebidas após 3 falhas. Agora: { $now }. A configuração do próprio servidor (ou o ServerAliveInterval dele) prevalece.

# off
tab3a8a0a = desativado

# known_hosts file
t2e38efc6 = Arquivo known_hosts

# Shared with ssh. shellwing adds the keys you trust.
t43d70395 = Compartilhado com o ssh. O shellwing adiciona as chaves em que você confia.

# ssh config file
t70767f0d = Arquivo de configuração do ssh

# Read only. Its hosts show in the host list.
tc5c7d789 = Somente leitura. Os servidores dele aparecem na lista de servidores.

# Agent
tf5a4f646 = Agente

# Keys in ssh-agent (Pageant or the OpenSSH agent on Windows).
t28b5b26a = Chaves no ssh-agent (Pageant ou o agente do OpenSSH no Windows).

# Sync settings too
ta578f67d = Sincronizar também as configurações

# The look (dark or light, themes, fonts), the terminal's and SSH's defaults, and logging. Each device keeps its own sync folder, log folder, local shell, editor and keyboard shortcuts.
t27d3016a = A aparência (escuro ou claro, temas, fontes), os padrões do terminal e do SSH, e os registros. Cada dispositivo mantém a própria pasta de sincronização, pasta de registros, shell local, editor e atalhos de teclado.

# Let a passphrase join new devices
t6906cf2c = Permitir que uma frase secreta adicione novos dispositivos

# A new device that syncs the same place types it instead of the recovery key or being approved here. It is kept there, sealed by the passphrase: anyone who can read the place can try to guess it, so it has to be hard to guess.
t637858c5 = Um novo dispositivo que sincroniza o mesmo local a digita em vez da chave de recuperação ou de ser aprovado aqui. A chave fica guardada lá, selada pela frase secreta: qualquer pessoa que consiga ler o local pode tentar adivinhá-la, então ela precisa ser difícil de adivinhar.

# Hosts kept on this device only (in their editor) stay out of it.
t06079f7b = Servidores mantidos somente neste dispositivo (no editor deles) ficam de fora.

# Or a place of your own
t7b88a644 = Ou um lugar seu

# Hosts, saved passwords, keys and snippets travel through a folder another app syncs (iCloud Drive, Syncthing, Dropbox), a WebDAV server, S3-compatible storage or Git. Everything there is encrypted with your recovery key; the server never sees a secret.
t99dee6c8 = Servidores, senhas salvas, chaves e trechos passam por uma pasta que outro app sincroniza (iCloud Drive, Syncthing, Dropbox), um servidor WebDAV, um armazenamento compatível com S3 ou Git. Tudo lá é criptografado com a sua chave de recuperação; o servidor nunca vê um segredo.

# When it syncs
t4849994e = Ao sincronizar

# With {shown}
tabf6bc68 = Com { $shown }

# Show the recovery key
t16f62979 = Mostrar a chave de recuperação

# Stop syncing
t59fdbd70 = Parar de sincronizar

# Your place
t63cb5f41 = Seu lugar

# A folder
te84ceb50 = Uma pasta

# S3-compatible
tdfb04144 = Compatível com S3

# shellwing server
t5fee1d8d = Servidor shellwing

# iCloud Drive, Syncthing, a Dropbox or OneDrive folder, a network share.
tc7eed334 = iCloud Drive, Syncthing, uma pasta do Dropbox ou do OneDrive, um compartilhamento de rede.

# Choose a folder…
t6268d49f = Escolher uma pasta…

# Nextcloud, ownCloud, Synology, Koofr, Fastmail: the address of a folder there.
t5f763441 = Nextcloud, ownCloud, Synology, Koofr, Fastmail: o endereço de uma pasta lá.

# AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi: a bucket, and a key that can read and write it.
t179167f5 = AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi: um bucket e uma chave que possa lê-lo e gravá-lo.

# Endpoint
t27e7d69c = Endpoint

# Region (if it asks for one)
t23e4c7d3 = Região (se for pedida)

# Bucket
te34e3557 = Bucket

# Folder in the bucket
t4c3f6707 = Pasta no bucket

# Access key
t837b43b8 = Chave de acesso

# Secret key
t47252d7a = Chave secreta

# A shellwing server your company or you run: you sign in in the browser. It keeps your data encrypted and never has your keys.
t16c496e4 = Um servidor shellwing gerido por si ou pela sua empresa: inicia sessão no navegador. Guarda os seus dados encriptados e nunca tem as suas chaves.

# A private repository on GitHub, GitLab, Gitea or your own server, through this system's git: it signs in as it does for you (its credential manager, your SSH keys and agent). Each change is a commit.
t33e11efe = Um repositório privado no GitHub, GitLab, Gitea ou no seu próprio servidor, pelo git deste sistema: ele entra como faz para você (o gerenciador de credenciais, suas chaves SSH e o agente). Cada alteração é um commit.

# Git keeps every version: images in notes stay in the repository's history, encrypted, even after they are removed, so it only grows.
t7d448954 = O Git guarda todas as versões: as imagens das notas ficam cifradas no histórico do repositório, mesmo depois de removidas, por isso ele só cresce.

# Repository
te763c8b5 = Repositório

# Branch
ta3e27665 = Branch

# Token (if git cannot sign in by itself)
t7919b94b = Token (se o git não conseguir entrar sozinho)

# The token's user
tb1ad1226 = Usuário do token

# The password, secret key or token stays on this device, in the vault; other devices sign in with their own.
t2527393b = A senha, a chave secreta ou o token fica neste dispositivo, no cofre; outros dispositivos entram com os seus.

# Installing git…
t77685894 = Instalando o git…

# Sign in in your browser, then come back here.
t53007abd = Inicie sessão no navegador e volte aqui.

# macOS installs it with Apple's Command Line Tools, in a dialog of its own.
t4d2669ba = O macOS o instala com as Command Line Tools da Apple, em uma janela própria.

# Install git…
te826846c = Instalar o git…

# Windows installs it with winget, and asks to allow it.
t1f4d87c1 = O Windows o instala com o winget e pede permissão.

# Download git
te5143d09 = Baixar o git

# Install it in a terminal, then Connect again:
t6e62c13a = Instale-o em um terminal e depois conecte de novo:

# Install a newer git (git-scm.com), then Connect again.
t9a84a209 = Instale um git mais recente (git-scm.com) e depois conecte de novo.

# None saved yet.
t0c60ecfc = Nada salvo ainda.

# {n} saved. | {n} saved.
t2764bcb7 =
    { $n ->
        [one] { $n } salva.
       *[other] { $n } salvas.
    }

# The vault is not open.
t3636924b = O cofre não está aberto.

# Locked: its passphrase or recovery key opens it.
t351c2ed0 = Bloqueado: a frase secreta ou a chave de recuperação o abre.

# Not open.
tcd6d61d0 = Não aberto.

# In the system keychain, and kept by a passphrase too.
tc986f88d = No chaveiro do sistema, e protegido também por uma frase secreta.

# Kept by its passphrase: typed when shellwing starts.
t31c62b2e = Protegido pela frase secreta: digitada quando o shellwing inicia.

# In the system keychain.
te1cb09c0 = No chaveiro do sistema.

# Open, but nothing keeps its key: set a passphrase.
teba6e9a1 = Aberto, mas nada guarda a chave dele: defina uma frase secreta.

# Unlock…
tc671ed6b = Desbloquear…

# Change the passphrase…
t9653b30a = Alterar a frase secreta…

# Set a passphrase…
tc40202f8 = Definir uma frase secreta…

# Lock now
t1aa78686 = Bloquear agora

# Ask for it at every start
t1535bdfc = Pedir a cada início

# Remove the passphrase
tc153db4a = Remover a frase secreta

# Keep it in the system keychain
tc01bbcf0 = Guardar no chaveiro do sistema

# {m} min
t7eebb6a4 = { $m } min

# When shellwing starts, and after this long without a key pressed or a click, its windows hide until Touch ID or your Mac's password says it is you. A light guard for a screen left unlocked: the sessions go on underneath.
ta9231ed4 = Quando o shellwing inicia, e após esse tempo sem uma tecla pressionada ou um clique, as janelas dele ficam ocultas até que o Touch ID ou a senha do seu Mac confirme que é você. Uma proteção leve para uma tela deixada desbloqueada: as sessões continuam por baixo.

# When shellwing starts, and after this long without a key pressed or a click, its windows hide until Windows Hello or your Windows password says it is you. A light guard for a screen left unlocked: the sessions go on underneath.
t34186d03 = Quando o shellwing inicia, e após esse tempo sem uma tecla pressionada ou um clique, as janelas dele ficam ocultas até que o Windows Hello ou sua senha do Windows confirme que é você. Uma proteção leve para uma tela deixada desbloqueada: as sessões continuam por baixo.

# When shellwing starts, and after this long without a key pressed or a click, its windows hide until your login password says it is you. A light guard for a screen left unlocked: the sessions go on underneath.
teb40e26b = Quando o shellwing inicia, e após esse tempo sem uma tecla pressionada ou um clique, as janelas dele ficam ocultas até que sua senha de login confirme que é você. Uma proteção leve para uma tela deixada desbloqueada: as sessões continuam por baixo.

# Lock shellwing when away
t7ad7e7d1 = Bloquear o shellwing quando você se ausentar

# Saved passwords and passphrases
t4b07c4a2 = Senhas e frases secretas salvas

# Offered for saving after you type one that works, and tried before asking next time. Kept in vault.db, encrypted.
ta9eeda13 = Oferecidas para salvar depois que você digita uma que funciona, e tentadas antes de perguntar na próxima vez. Guardadas em vault.db, criptografadas.

# Manage…
t4f2e18b6 = Gerenciar…

# The vault's key
td09ac71d = A chave do cofre

# What opens vault.db. A passphrase keeps it where there is no system keychain (or as well as one); the recovery key always opens it, if the passphrase is forgotten or the keychain loses it.
t6a54a5a7 = O que abre o vault.db. Uma frase secreta a guarda onde não há chaveiro do sistema (ou além dele); a chave de recuperação sempre o abre, se a frase secreta for esquecida ou o chaveiro a perder.

# When the vault is locked, {name} opens it, as its passphrase does (which still works). The fingers enrolled now: adding one stops it, until it is turned on again. Needs a signed build of shellwing.
t056f7fb9 = Quando o cofre está bloqueado, { $name } o abre, como a frase secreta (que continua funcionando). Valem os dedos cadastrados agora: cadastrar outro desativa o recurso até ele ser ativado de novo. Requer uma versão assinada do shellwing.

# When the vault is locked, {name} (a fingerprint, the face or this device's PIN) opens it, as its passphrase does (which still works). The key is kept by this device's security chip.
t0baa8f35 = Quando o cofre está bloqueado, { $name } (uma impressão digital, o rosto ou o PIN deste dispositivo) o abre, como a frase secreta (que continua funcionando). A chave é guardada pelo chip de segurança deste dispositivo.

# Unlock with {name}
t7c964cc4 = Desbloquear com { $name }

# Lock when idle
ta18da274 = Bloquear quando ocioso

# After this long without a key pressed or a click, the vault locks until its passphrase is typed. Only while the system keychain does not hold its key: each start asks for the passphrase then.
tf6c5f76d = Após esse tempo sem uma tecla pressionada ou um clique, o cofre é bloqueado até a frase secreta ser digitada. Somente enquanto o chaveiro do sistema não guarda a chave dele: nesse caso, cada início pede a frase secreta.

# Open config.toml
t07a52335 = Abrir config.toml

# Filter by name, tag or address
t84bd523e = Filtrar por nome, tag ou endereço

# connected
t54eed4d0 = conectado

# connecting
t5fb60aa7 = conectando

# connection lost
te76d62e9 = conexão perdida

# could not connect
t87553579 = não foi possível conectar

# closed
tebee50c5 = fechado

# One command, many hosts
t05dcb762 = Um comando, vários servidores

# ACTIONS
t62d7de84 = AÇÕES

# What AI clients run
t81fb801a = O que os clientes de AI executam

# Cmd+click to add or take away
t36133970 = Cmd+click para adicionar ou tirar

# Ctrl+click to add or take away
t47785603 = Ctrl+click para adicionar ou tirar

# Give to AI…
ta74a1e27 = Conceder à AI…

# Run on the {n} host of {group} | Run on the {n} hosts of {group}
t4ecd858a =
    { $n ->
        [one] Executar no { $n } servidor de { $group }
       *[other] Executar nos { $n } servidores de { $group }
    }

# No hosts yet
ta58f36ff = Nenhum servidor ainda

# No host matches.
t2a8f8871 = Nenhum servidor corresponde.

# No hosts yet. Add one, or Host blocks to ~/.ssh/config.
tbbe50baa = Nenhum servidor ainda. Adicione um, ou blocos Host em ~/.ssh/config.

# Import hosts from another app…
tb00d665f = Importar servidores de outro app…

# + Host
t0e963f08 = + Servidor

# + Group
t9585c651 = + Grupo

# About shellwing
t884dc61f = Sobre o shellwing

# HOSTS
t0d325d34 = SERVIDORES

# New snippet
tff3f7838 = Novo trecho

# What it does, to find it in the palette
td9b8e02f = O que ele faz, para encontrá-lo na paleta

# A name and a command are needed.
t7f4c69bb = É preciso um nome e um comando.

# this snippet
t155d1e08 = este trecho

# Every host
t77846bfe = Todos os servidores

# A group ▾
tcd864ec5 = Um grupo ▾

# One host ▾
t1864f915 = Um servidor ▾

# Offered for
tbafebe63 = Oferecido para

# No groups yet: make one in the host list.
t31c54e54 = Nenhum grupo ainda: crie um na lista de servidores.

# The palette lists it in panes of those hosts only.
t66336218 = A paleta o lista somente em painéis desses servidores.

# (deleted)
tb7bba50b = (excluído)

# + Add
ta0ef59dd = + Adicionar

# Environments it needs
tebd9fda9 = Ambientes de que precisa

# {n} variable | {n} variables
te4598f62 =
    { $n ->
        [one] { $n } variável
       *[other] { $n } variáveis
    }

# It runs in a subshell that has their variables; they go when it ends.
td5ccd9da = Ele roda em um subshell que tem as variáveis deles; elas somem quando ele termina.

# Values
t8d4d225b = Valores

# from the pane's host
t0607dcd8 = do servidor do painel

# asked when it runs
t8ee70b94 = perguntado ao executar

# {{host}}, {{user}} and {{address}} are filled in; any other {{name}} is asked for when it runs.
t482910da = {"{{"}host{"}}"}, {"{{"}user{"}}"} e {"{{"}address{"}}"} são preenchidos; qualquer outro {"{{"}name{"}}"} é pedido quando ele é executado.

# Run it: press Enter after typing it
t32122ee4 = Executar: pressionar Enter após digitá-lo

# Off: it is typed, and waits for you to read it.
t2ccc04b4 = Desligado: ele só é digitado e espera você ler.

# {n} line | {n} lines
t5c4bb67a =
    { $n ->
        [one] { $n } linha
       *[other] { $n } linhas
    }

# Run in {pane}
t27e0e0f9 = Executar em { $pane }

# Connect a tab in the main window to run it there.
tc580324f = Conecte uma aba na janela principal para executá-lo lá.

# {n} byte | {n} bytes
te8f231ba =
    { $n ->
        [one] { $n } byte
       *[other] { $n } bytes
    }

# {n} MB
t3258fa2a = { $n } MB

# {n} KB
te253ff0c = { $n } KB

# The file's text
t7fb1322b = O texto do arquivo

# What it is, such as kube prod
t2c4b7067 = O que é, por exemplo kube prod

# It is not text, or longer than {max} KB: replace it from a file instead.
t70f95162 = Não é texto, ou tem mais de { $max } KB: substitua-o a partir de um arquivo.

# Where it goes: a path from / or from ~/, such as ~/.kube/config.
t2b706ff4 = Onde ele vai: um caminho começando por / ou por ~/, por exemplo ~/.kube/config.

# Where it goes: a file's path, not a folder's.
t0038f509 = Onde ele vai: o caminho de um arquivo, não de uma pasta.

# Permissions: three octal digits, such as 600.
t5dde13fd = Permissões: três dígitos octais, por exemplo 600.

# {size}, not text
t5956f56a = { $size }, não é texto

# Edit…
t8e7efcd7 = Editar…

# It goes from the vault, and from your other devices once they sync. Where it was placed, it stays.
t9903435f = Ele sai do cofre e dos seus outros dispositivos assim que eles sincronizarem. Onde já foi colocado, ele fica.

# Choose a file…
t28a576c9 = Escolher um arquivo…

# {file}: {size}, kept as it is
t9e564235 = { $file }: { $size }, mantido como está

# Content
tc1679f62 = Conteúdo

# Text up to {edited} is edited here; any file up to {max} can be taken from this computer.
t992520da = Texto de até { $edited } é editado aqui; qualquer arquivo de até { $max } pode ser trazido deste computador.

# Until the tab closes
t5ad5ff54 = Até a aba fechar

# To stay
t89fb6375 = Para ficar

# When placed
t82fcea82 = Ao colocar

# What placing it offers first, for Enter: taken away when the tab closes, or left on the host.
t9867bbe6 = O que colocar oferece primeiro, para Enter: removido quando a aba fechar, ou deixado no servidor.

# The palette offers to place it in panes of those hosts only.
tdc9acac2 = A paleta só oferece colocá-lo nos painéis desses servidores.

# Where it goes on the host
taf38972d = Onde ele vai no servidor

# Permissions
t8700fd83 = Permissões

# New config file
td0fa9fed = Novo arquivo de configuração

# Edit config file
t56c9aeed = Editar arquivo de configuração

# Category
tafb3e591 = Categoria

# The vault's passphrase, to show the values
teecb1d4f = Frase secreta do cofre, para mostrar os valores

# No passphrase guards the vault, so anyone at this device can show them. Show the values?
tc7928fa4 = Nenhuma frase secreta protege o cofre, então qualquer pessoa neste dispositivo pode mostrá-los. Mostrar os valores?

# value
t425ed3ca = valor

# What they are for, such as AWS production
tb620b1e5 = Para que servem, como AWS produção

# {name} is not a variable's name.
tdd2c4c4b = { $name } não é um nome de variável válido.

# {name} is there twice.
te1458d84 = { $name } aparece duas vezes.

# {name} has no value.
t387a8908 = { $name } não tem valor.

# Add a variable: a name and its value.
t6150163b = Adicione uma variável: um nome e seu valor.

# The clipboard holds no NAME=value lines.
tbf497c49 = A área de transferência não tem linhas NAME=value.

# Not NAME=value: {e}
t65a67e1e = Não é NAME=value: { $e }

# needed by {n} snippet | needed by {n} snippets
t79d75da2 =
    { $n ->
        [one] necessário para { $n } trecho
       *[other] necessário para { $n } trechos
    }

# Its variables go from the vault, and from your other devices once they sync. Shells it is loaded in keep them until unloaded.
td2d90383 = Suas variáveis saem do cofre e dos seus outros dispositivos assim que eles sincronizarem. Os shells em que ele está carregado as mantêm até que seja descarregado.

# The palette offers to load it in panes of those hosts only.
t67715aad = A paleta oferece carregá-lo apenas nos painéis desses servidores.

# Variables
t7996f062 = Variáveis

# Add a variable
tf53dee25 = Adicionar variável

# Paste a .env file
tf0c57252 = Colar um arquivo .env

# A value can take several lines: Enter starts another, {save} saves.
ta46f6540 = Um valor pode ter várias linhas: Enter começa outra, { $save } salva.

# New environment
tf3955dbc = Novo ambiente

# Edit environment
t79646cbc = Editar ambiente

# sensitive
ta2bd2f4f = confidencial

# Start from the library
t401f9cb9 = Comece pela biblioteca

# {n} ready commands for {tools} and more: check a server, back it up, clean it, test it. Pick the ones you want; they become your own snippets, to edit as you like.
tffb7b170 = { $n } comandos prontos para { $tools } e mais: verificar um servidor, fazer backup, limpar, testar. Escolha os que quiser; eles viram seus próprios trechos, para editar à vontade.

# Open the library
t79d7b3ad = Abrir a biblioteca

# from the library
td908f0d1 = da biblioteca

# typed, not run
td4e41905 = digitado, não executado

# with {envs}
tba31f3bb = com { $envs }

# None yet.
t786bc275 = Nenhum ainda.

# Library…
t3d769c8c = Biblioteca…

# New snippet…
tec3111a4 = Novo trecho…

# New environment…
t0c42d278 = Novo ambiente…

# New config file…
t58dfdc93 = Novo arquivo de configuração…

# New note…
tdad71f9d = Nova nota…

# Variables for the shell, such as an API's keys, kept encrypted in the vault and synced with it. Load one into a pane from the palette ({chord}, under Environments) or the pane's menu, and unload it when done; or let a snippet that needs it have it for itself alone.
t7baa8160 = Variáveis para o shell, como as chaves de uma API, guardadas criptografadas no cofre e sincronizadas com ele. Carregue uma em um painel pela paleta ({ $chord }, em Ambientes) ou pelo menu do painel, e descarregue-a quando terminar; ou deixe um trecho que precise dela usá-la só para si.

# Commands you run often. Run one from the palette ({chord}, then type its name): Enter runs it in the pane, {alt} in every pane of the tab. Kept in the vault, so they sync with your hosts.
tb1ca142d = Comandos que você executa com frequência. Execute um pela paleta ({ $chord }, depois digite o nome): Enter o executa no painel, { $alt } em todos os painéis da aba. Guardados no cofre, por isso sincronizam com seus servidores.

# Free text, kept encrypted in the vault and synced with it: what a server is for, how to restart it, codes to keep. Each opens in a window of its own, from here or the palette ({chord}, under Notes); plain text or Markdown, saved as you type.
t5a3830ee = Texto livre, criptografado no cofre e sincronizado com ele: para que serve um servidor, como reiniciá-lo, códigos a guardar. Cada uma abre em uma janela própria, daqui ou da paleta ({ $chord }, em Notas); texto simples ou Markdown, salvo enquanto você digita.

# Files to place on hosts, such as ~/.kube/config or ~/.aws/credentials, kept encrypted in the vault and synced with it. Place one from the palette ({chord}, under Config files) or the pane's menu: until the tab closes, or to stay.
ta84c0f04 = Arquivos para colocar em servidores, como ~/.kube/config ou ~/.aws/credentials, guardados criptografados no cofre e sincronizados com ele. Coloque um pela paleta ({ $chord }, em Arquivos de configuração) ou pelo menu do painel: até a aba fechar, ou para ficar.

# More in the library: {n} not added yet › | More in the library: {n} not added yet ›
tf7e18b27 =
    { $n ->
        [one] Mais na biblioteca: { $n } ainda não adicionado ›
       *[other] Mais na biblioteca: { $n } ainda não adicionados ›
    }

# While loaded, the server can read them
te60c8275 = Enquanto estiverem carregadas, o servidor pode lê-las

# Values are sent with the echo off and kept out of the shell's history, but a shell that has them holds them in its memory: whoever has root on that server, or runs as your user there, can read them until they are unloaded or the shell ends.
t87b5db80 = Os valores são enviados com o eco desligado e ficam fora do histórico do shell, mas um shell que os tem os guarda na memória: quem tiver root nesse servidor, ou rodar como seu usuário lá, pode lê-los até que sejam descarregados ou o shell termine.

# Load them on servers you trust, unload them when done, and prefer keys that expire or can do little, such as short-lived tokens or a role with only what the work needs.
t16991c40 = Carregue-as em servidores de confiança, descarregue ao terminar e prefira chaves que expirem ou que possam pouco, como tokens de curta duração ou um papel com apenas o que o trabalho precisa.

# A placed file is on the server's disk, with its permissions: whoever has root there can read it until it is removed. Place secrets on servers you trust, until the tab closes.
t1d5d16cc = Um arquivo colocado fica no disco do servidor, com suas permissões: quem for root lá pode lê-lo até ele ser removido. Coloque segredos só em servidores de confiança, e até a aba fechar.

# {{host}}, {{user}} and {{address}} are filled in for the pane's host; any other {{name}} is asked for when the snippet runs.
t95cfcc45 = {"{{"}host{"}}"}, {"{{"}user{"}}"} e {"{{"}address{"}}"} são preenchidos com os dados do servidor do painel; qualquer outro {"{{"}name{"}}"} é pedido quando o trecho é executado.

# {n} password | {n} passwords
tf07ec59c =
    { $n ->
        [one] { $n } senha
       *[other] { $n } senhas
    }

# {n} snippet | {n} snippets
t4a5f7c7a =
    { $n ->
        [one] { $n } trecho
       *[other] { $n } trechos
    }

# {n} environment | {n} environments
t97db2cda =
    { $n ->
        [one] { $n } ambiente
       *[other] { $n } ambientes
    }

# {n} config file | {n} config files
tc9aa46c2 =
    { $n ->
        [one] { $n } arquivo de configuração
       *[other] { $n } arquivos de configuração
    }

# {n} note | {n} notes
t9adc29da =
    { $n ->
        [one] { $n } nota
       *[other] { $n } notas
    }

# NOT SYNCING
t7c7f0ba3 = SEM SINCRONIZAÇÃO

# Your hosts on every device. Just sign in.
t88dd5f11 = Seus servidores em todos os dispositivos. Basta entrar.

# Hosts, passwords, keys and snippets stay on this computer for now.
td2cff7db = Servidores, senhas, chaves e trechos ficam neste computador por enquanto.

# See shellwing cloud
t88cbb4e0 = Conhecer o shellwing cloud

# A folder, WebDAV, S3 or Git of your own:
t42ad626a = Uma pasta, WebDAV, S3 ou Git próprio:

# Settings › Sync
t6790eaeb = Configurações › Sincronização

# SYNC
t476c685a = SINCRONIZAÇÃO

# Open Settings › Sync
tbef0fba1 = Abrir Configurações › Sincronização

# Synced · {when}
ta250b3ba = Sincronizado · { $when }

# With {place}
t6c9fa51a = Com { $place }

# Set up
t43c06abc = Configurar

# Syncing…
t8ad2ae06 = Sincronizando…

# Not synced
t4d1a6724 = Não sincronizado

# Paused: the vault is locked
tfff0fb2a = Pausado: o cofre está bloqueado

# With {place}, once it is open
tf75bc464 = Com { $place }, assim que ele for aberto

# Sync now
ta34dd8fe = Sincronizar agora

# Hosts, saved passwords, keys and snippets follow you to every computer.
t025d99af = Servidores, senhas salvas, chaves e trechos acompanham você em todos os computadores.

# Nothing to set up: no folder, no server, no key to type on a new device.
tde7b1d58 = Nada a configurar: nenhuma pasta, nenhum servidor, nenhuma chave para digitar em um novo dispositivo.

# Encrypted on your devices: shellwing never sees your secrets.
t35aecaf3 = Criptografado nos seus dispositivos: o shellwing nunca vê seus segredos.

# Create an account
td7f6c093 = Criar uma conta

# or continue with
t2f60c9bf = ou continue com

# Rather not keep it yourself?
t616f184a = Prefere não guardar isso você mesmo?

# shellwing cloud keeps it for you: sign in on each device, and that is all. Still end-to-end encrypted.
t81dee99e = O shellwing cloud guarda para você: entre em cada dispositivo e pronto. Continua com criptografia de ponta a ponta.

# shellwing cloud is coming: signing in opens with it.
t8de35792 = O shellwing cloud está chegando: o login abre com ele.

# {n} host · coming soon | {n} hosts · coming soon
t08904d1a =
    { $n ->
        [one] { $n } servidor · em breve
       *[other] { $n } servidores · em breve
    }

# unlimited · coming soon
t7c4d88dd = ilimitado · em breve

# just now
ta715ef0f = agora mesmo

# a minute ago
t85ac693d = há um minuto

# {n} minute ago | {n} minutes ago
t6c7302d6 =
    { $n ->
        [one] há { $n } minuto
       *[other] há { $n } minutos
    }

# an hour ago
t84377069 = há uma hora

# {n} hour ago | {n} hours ago
t137b052e =
    { $n ->
        [one] há { $n } hora
       *[other] há { $n } horas
    }

# {n} day ago | {n} days ago
tb9fa7744 =
    { $n ->
        [one] há { $n } dia
       *[other] há { $n } dias
    }

# Find a tab: host, user, title
t187bd0b5 = Encontre uma aba: servidor, usuário, título

# {n} tab | {n} tabs
t09a24eca =
    { $n ->
        [one] { $n } aba
       *[other] { $n } abas
    }

# {n} pane | {n} panes
tb53e7b62 =
    { $n ->
        [one] { $n } painel
       *[other] { $n } painéis
    }

# this tab
tfd01d10a = esta aba

# Tab {number}
te154cf6f = Aba { $number }

# No tab matches.
t9b01e94e = Nenhuma aba corresponde.

# ↑↓ choose
t983339a6 = ↑↓ escolher

# Enter go to it
td90b12ab = Enter ir para ela

# Cmd+W close it
t6246d272 = Cmd+W fechá-la

# Ctrl+W close it
t82a6adaf = Ctrl+W fechá-la

# Find
t3e1cd17a = Localizar

# Invalid pattern
t5b9dcaca = Padrão inválido

# No results
tdd4785bc = Nenhum resultado

# {current} of {total}{more}
ta6206114 = { $current } de { $total }{ $more }

# {total} found{more}
t59fb4764 = { $total } encontrados{ $more }

# ↓ Back to bottom (Shift+End)
t5a2b7304 = ↓ Voltar ao final (Shift+End)

# A full-screen program is running in this pane: leave it, then try again.
t20301272 = Um programa em tela cheia está rodando neste painel: saia dele e tente de novo.

# Removed {n} file. | Removed {n} files.
t0f179fa4 =
    { $n ->
        [one] { $n } arquivo removido.
       *[other] { $n } arquivos removidos.
    }

# Not kept: {e}
tc12e4203 = Não guardada: { $e }

# {path} stays when the tab closes.
t4d349d57 = { $path } fica quando a aba fechar.

# {n} file stays when the tab closes. | {n} files stay when the tab closes.
t500558bd =
    { $n ->
        [one] { $n } arquivo fica quando a aba fechar.
       *[other] { $n } arquivos ficam quando a aba fechar.
    }

# What was there before is {path}.
td07a4718 = O que havia antes agora é { $path }.

# {name} is loaded into this shell.
t5bc879d1 = { $name } está carregado neste shell.

# Log saved: {path}
tcc43a8d9 = Registro salvo: { $path }

# Logging to {path}
td2342a94 = Registrando em { $path }

# Cannot log: {e}
tb673f869 = Não é possível registrar: { $e }

# Saved as HTML: {path}
t21558f46 = Salvo como HTML: { $path }

# Saved as text: {path}
t17977298 = Salvo como texto: { $path }

# Keep
t555cbe68 = Manter

# logging
t897ecb9e = registrando

# listening
t77104bea = recebendo

# Installing {key} on {host}…
t9e6d8462 = Instalando { $key } em { $host }…

# the connection ended
t36d02558 = a conexão foi encerrada

# {key} is installed on {host}.
t4ae79143 = { $key } está instalada em { $host }.

# {host} has {key} already.
t12a2b3c2 = { $host } já tem { $key }.

# {key} was not installed on {host}: {why}
t25610553 = { $key } não foi instalada em { $host }: { $why }

# Connected without what these hosts of an organization's collection ask. Whoever may edit the collection sets it, and it acts on this computer: allow it only if you trust them.
t108152c1 = Conectado sem o que estes servidores da coleção de uma organização pedem. Quem pode editar a coleção define isso, e age neste computador: permita só se confiar nessas pessoas.

# Not now
t2c3d6fce = Agora não

# Allow and reconnect
t6f3784ee = Permitir e reconectar

# {key} is installed on {host}. Sign in to {host} with it from now on?
ta04e62fb = { $key } está instalada em { $host }. Entrar em { $host } com ela daqui em diante?

# Use it for {host}
t704df588 = Usar para { $host }

# Match case (Alt+C)
t692d685e = Diferenciar maiúsculas e minúsculas (Alt+C)

# Regular expression (Alt+R)
tc1b10302 = Expressão regular (Alt+R)

# The Files panel browses a server over SFTP; this pane is a shell on this computer.
t0e83f1c9 = O painel Arquivos navega por um servidor via SFTP; este painel é um shell neste computador.

# Saved in the vault.
t0d69c4e6 = Salva no cofre.

# Save the password for {target} in the vault?
tceb19137 = Salvar a senha de { $target } no cofre?

# Save the passphrase of {key_path} in the vault?
tc75d3456 = Salvar a frase secreta de { $key_path } no cofre?

# The host key for {host} has changed.
t1e510d66 = A chave do servidor { $host } mudou.

# If the server was reinstalled or its key replaced, trust the new key. If you do not know why it changed, do not: someone may be between you and the server, reading what you send.
t26f8b8a1 = Se o servidor foi reinstalado ou a chave dele foi substituída, confie na nova chave. Se você não sabe por que ela mudou, não confie: alguém pode estar entre você e o servidor, lendo o que você envia.

# Known
t3f42d62c = Conhecida

# {old}  (known_hosts line {line})
t8797a1b8 = { $old }  (linha { $line } de known_hosts)

# Now
t8b3dc597 = Agora

# Trust the new key and connect
t48aebd90 = Confiar na nova chave e conectar

# Not trusted: {e}
tdfcff622 = Não confiada: { $e }

# Connection lost. Trying again in {seconds} s (attempt {attempt} of {attempts}).
t63f1b6c3 = Conexão perdida. Tentando de novo em { $seconds } s (tentativa { $attempt } de { $attempts }).

# Gave up after {n} attempt. Enter tries again. | Gave up after {n} attempts. Enter tries again.
t446dfb2c =
    { $n ->
        [one] Desistiu após { $n } tentativa. Enter tenta de novo.
       *[other] Desistiu após { $n } tentativas. Enter tenta de novo.
    }

# Connection lost. Enter tries again.
t1ca41f48 = Conexão perdida. Enter tenta de novo.

# {why}. Enter starts it again.
tcad1f44f = { $why }. Enter inicia de novo.

# {why}. Enter reconnects.
t9788f089 = { $why }. Enter reconecta.

# Could not connect. Enter tries again.
tfbba91f8 = Não foi possível conectar. Enter tenta de novo.

# Stop (Esc)
t47aaed65 = Parar (Esc)

# Start again (Enter)
tbfd489be = Iniciar de novo (Enter)

# Reconnect (Enter)
t06029ac3 = Reconectar (Enter)

# Font size {size}
t3fc7bcbc = Tamanho da fonte { $size }

# The pasted text could run commands. Paste again within 5 s to send it anyway.
t6e9aeddb = O texto colado pode executar comandos. Cole de novo em até 5 s para enviá-lo mesmo assim.

# Logging stopped: {e}
t435e7ac0 = Registro interrompido: { $e }

# Command Palette…
t14aa6ff7 = Paleta de comandos…

# Snippets…
t2b3c5b61 = Trechos…

# Load Environment…
td34b2cc0 = Carregar ambiente…

# Place Config File…
t49f82f0c = Colocar arquivo de configuração…

# Keep a File in the Vault…
t25c07703 = Guardar um arquivo no cofre…

# Keep {names} on the host
t80142336 = Manter { $names } no servidor

# Remove {names}
t4efeb00b = Remover { $names }

# Unload {names}
t362b9a48 = Descarregar { $names }

# Find…
t1ecb565a = Localizar…

# Hide Files
t55bc30ae = Ocultar arquivos

# Browse Files (SFTP)
tf41b1da0 = Navegar pelos arquivos (SFTP)

# Forward a Port…
te7550adc = Redirecionar uma porta…

# Save Output as Text…
tffbf1682 = Salvar saída como texto…

# Save Output as HTML…
t30c151b0 = Salvar saída como HTML…

# with colors
te0d866bb = com cores

# Stop Logging
td281e470 = Parar registro

# Start Logging
t72c41b76 = Iniciar registro

# Split Right
t50db06c1 = Dividir à direita

# Split Down
t1165de29 = Dividir abaixo

# Close Pane
t21a947cf = Fechar painel

# {client} wants to run a command on {host}: AI activity has it, waiting for you.
tc4369c9f = { $client } quer executar um comando em { $host }: ele está em Atividade de AI, esperando por você.

# Paused: {client} waits until you let it go on.
t56154080 = Pausado: { $client } espera até você o deixar continuar.

# {client} is using this tab: {n} command so far. | {client} is using this tab: {n} commands so far.
ta805a0f4 =
    { $n ->
        [one] { $client } está usando esta aba: { $n } comando até agora.
       *[other] { $client } está usando esta aba: { $n } comandos até agora.
    }

# Let it go on
t6f36d148 = Deixar continuar

# Take access back
td619baa7 = Revogar acesso

# Config files · {category}
t59edc3e9 = Arquivos de configuração · { $category }

# placed: Enter places it again
taf8f6d41 = colocado: Enter o coloca de novo

# {name} · {size} · permissions {mode}
ta22bb64e = { $name } · { $size } · permissões { $mode }

# Where on {host}
t0f76d445 = Onde em { $host }

# Cannot place {name}: {e}
t1383f29b = Não foi possível colocar { $name }: { $e }

# Replace the one placed here until the tab closes
t5ca1c37e = Substituir o colocado aqui até a aba fechar

# What was there before still comes back when the tab closes.
t55130ddc = O que havia antes volta quando a aba fechar.

# Keep the one placed here
t840a8b5a = Manter o colocado aqui

# It stays when the tab closes.
tb2bc8f3b = Ele fica quando a aba fechar.

# {path} holds it already.
t3ec4172b = { $path } já o contém.

# Place it to stay
t698a38cf = Colocar para ficar

# Place it until the tab closes
t19537c52 = Colocar até a aba fechar

# Then it is taken away.
t1302797a = Depois ele é retirado.

# Replace it, keeping the old one
tc95f7291 = Substituir, mantendo o antigo

# As {path}
tfb10a76a = Como { $path }

# Replace it
t1e17fc0c = Substituir

# The one there now is gone.
t7fa127b9 = O que está lá agora se perde.

# Replace the file there until the tab closes
t7e646019 = Substituir o arquivo de lá até a aba fechar

# The one there now is set aside, and comes back when the tab closes.
t8c121382 = O que está lá agora é posto de lado e volta quando a aba fechar.

# {path} is there already…
td716b366 = { $path } já existe…

# Place at {path}…
tdcc97a76 = Colocar em { $path }…

# Placed {name} at {path} until the tab closes.
t59328aae = { $name } colocado em { $path } até a aba fechar.

# Placed {name} at {path}.
tfce33e2b = { $name } colocado em { $path }.

# Took away {n} config file left here from before. | Took away {n} config files left here from before.
ta674430e =
    { $n ->
        [one] { $n } arquivo de configuração que sobrou de antes foi retirado.
       *[other] { $n } arquivos de configuração que sobraram de antes foram retirados.
    }

# Left {path} as it is: it changed since it was placed.
tccbb9d92 = { $path } foi deixado como está: mudou desde que foi colocado.

# Kept encrypted as a config file, for this host, up to {max} KB.
tf7742770 = Guardado criptografado como arquivo de configuração, para este servidor, até { $max } KB.

# A file on {host} to keep in the vault
t63904f7b = Um arquivo em { $host } para guardar no cofre

# Kept {path} in the vault: see Settings › Config files.
t8589664d = { $path } guardado no cofre: veja Configurações › Arquivos de configuração.

# Removing the config files placed…
tbd123099 = Removendo os arquivos de configuração colocados…

# Open a tab first
t6460fddf = Abra uma aba primeiro

# Connect first
t0b55252f = Conecte-se primeiro

# Notes for {host}
ta77bd589 = Notas para { $host }

# Notes · {category}
tbb112cdf = Notas · { $category }

# The window closed.
t26993482 = A janela foi fechada.

# its collection's settings for {names} wait until you allow them, on a tab to it
t3306232b = as configurações da coleção dele para { $names } aguardam até você permiti-las, em uma aba para ele

# Stop and close
tb789bdac = Parar e fechar

# Close the tab?
tc626ae98 = Fechar a aba?

# It is still running: closing stops it on the hosts it has not ended on.
t853369a2 = Ainda está em execução: fechar o interrompe nos servidores em que ele não terminou.

# the system's package manager
t11465c20 = o gerenciador de pacotes do sistema

# the App Store
ta8cbd72e = a App Store

# the Microsoft Store
t1df799a1 = a Microsoft Store

# shellwing {version} is out: {by} updates it ({command}).
t270e30ba = shellwing { $version } está disponível: atualize usando { $by } ({ $command }).

# shellwing {version} is out: {by} updates it.
t34c8e3f2 = shellwing { $version } está disponível: atualize usando { $by }.

# No folder to download to.
t2bfc1370 = Não há pasta para onde baixar.

# shellwing {version} is ready: it is installed when you quit, or now with Restart.
t4cc4357e = shellwing { $version } está pronto: ele é instalado quando você sai, ou agora com Reiniciar.

# Update
t6e230e94 = Atualizar

# Downloading… {got} of {total}
taee668c2 = Baixando… { $got } de { $total }

# Downloading… {got}
t4873b921 = Baixando… { $got }

# New host…
t13806b73 = Novo servidor…

# Manage access…
t4003f650 = Gerenciar o acesso…

# Give its key…
t5f2231e7 = Dar a chave…

# Secrets to change…
tdba41f83 = Segredos a trocar…

# Over SSH only
t14e43465 = Só por SSH

# Snippets · {category}
t85f58d76 = Trechos · { $category }

# Enter · Alt+Enter: {n} pane | Enter · Alt+Enter: {n} panes
t3bbceccc =
    { $n ->
        [one] Enter · Alt+Enter: { $n } painel
       *[other] Enter · Alt+Enter: { $n } painéis
    }

# Snippet library…
tff254ce3 = Biblioteca de trechos…

# {n} ready command to add | {n} ready commands to add
t134e12f0 =
    { $n ->
        [one] { $n } comando pronto para adicionar
       *[other] { $n } comandos prontos para adicionar
    }

# Open a tab first · Alt+Enter: add…
tcd0bb8f7 = Abra uma aba primeiro · Alt+Enter: adicionar…

# Enter types it · Alt+Enter: add…
t7556b445 = Enter digita · Alt+Enter: adicionar…

# Library · {tool}
t8f5c1ee7 = Biblioteca · { $tool }

# Open a tab first: a snippet runs in the focused pane.
t45260fd2 = Abra uma aba primeiro: um trecho roda no painel selecionado.

# in this pane
t57139bb8 = neste painel

# in {n} pane | in {n} panes
tca97c422 =
    { $n ->
        [one] em { $n } painel
       *[other] em { $n } painéis
    }

# {snippet} · {panes} · {name} ({at} of {total})
t6748f755 = { $snippet } · { $panes } · { $name } ({ $at } de { $total })

# Will run
teaaf5750 = Será executado

# An environment this needs was deleted: edit the snippet.
tb37e7456 = Um ambiente de que este trecho precisa foi excluído: edite o trecho.

# loaded: Enter loads it again
t1a97d76a = carregado: Enter o carrega de novo

# Split the tab first: there is one pane to type in.
t72699bd1 = Divida a aba primeiro: há só um painel para digitar.

# Close the pane?
tf2b43783 = Fechar o painel?

# The connection to {one} is open; closing ends it.
t7958d376 = A conexão com { $one } está aberta; fechar a encerra.

# {n} connection is open; closing ends it. | {n} connections are open; closing ends them.
tbac763bb =
    { $n ->
        [one] { $n } conexão está aberta; fechar a encerra.
       *[other] { $n } conexões estão abertas; fechar as encerra.
    }

# Not allowed: {e}
t839d4e8b = Não permitido: { $e }

# Cannot open a terminal: {e}
t3ffc1c98 = Não é possível abrir um terminal: { $e }

# shellwing cannot lock here: this system has no way to ask that it is you.
t222eeef4 = O shellwing não pode se bloquear aqui: este sistema não tem como confirmar que é você.

# A connection is open; quitting closes it.
tf3732b18 = Uma conexão está aberta; sair a fecha.

# {n} connection is open; quitting closes it. | {n} connections are open; quitting closes them.
td41065cb =
    { $n ->
        [one] { $n } conexão está aberta; sair a fecha.
       *[other] { $n } conexões estão abertas; sair as fecha.
    }

# Restart
t2f44ed8c = Reiniciar

# Restart to install the update?
tdab4b0cf = Reiniciar para instalar a atualização?

# Quit shellwing?
tfa31f588 = Sair do shellwing?

# Syncing the notes before quitting…
tb66945de = Sincronizando as notas antes de sair…

# Choose a host on the left to connect.
tf7759ebb = Escolha um servidor à esquerda para conectar.

# Unlock the vault…
tdf659182 = Desbloquear o cofre…

# Snippets, environments and config files are in it.
t1646a763 = Os trechos, ambientes e arquivos de configuração estão nele.

# Another tab to {title}
ta46c00f0 = Outra aba para { $title }

# Split right, to {title}
tef0930dc = Dividir à direita, para { $title }

# Split down, to {title}
tfe6ae2e4 = Dividir abaixo, para { $title }

# Close the tab
t1c6c1637 = Fechar a aba

# Stop typing in every pane
tfa86f71a = Parar de digitar em todos os painéis

# Type in every pane of this tab
t047bdf97 = Digitar em todos os painéis desta aba

# All {count} tabs, by host…
t4780291e = Todas as { $count } abas, por servidor…

# Find in {title}…
t9f86553f = Localizar em { $title }…

# Save the output of {title} as text…
t8947128e = Salvar a saída de { $title } como texto…

# Save the output of {title} as HTML, with colors…
t1c2d8b8c = Salvar a saída de { $title } como HTML, com cores…

# Stop logging {title}
t87730020 = Parar o registro de { $title }

# Log {title} to a file
t2f1b2b25 = Registrar { $title } em um arquivo

# Hide the files
td567c539 = Ocultar os arquivos

# Files on {title}
t13c5bc91 = Arquivos em { $title }

# Forward a port through {title}…
t9f033387 = Redirecionar uma porta via { $title }…

# Unload environment: {names}
tbe80f7b5 = Descarregar ambiente: { $names }

# Run on hosts…
t8ceaeff0 = Executar em servidores…

# Change theme…
t85a401b4 = Alterar tema…

# Hide the host list
td7560c88 = Ocultar a lista de servidores

# Show the host list
t4754a8f9 = Mostrar a lista de servidores

# Saved passwords and passphrases…
tdfc17c92 = Senhas e frases secretas salvas…

# Lock the vault
tc05449a1 = Bloquear o cofre

# Lock shellwing
ta4580143 = Bloquear o shellwing

# Sync now with {place}
t8aad97ad = Sincronizar agora com { $place }

# Secrets to change in {name}…
t6dbdbc5f = Segredos a trocar em { $name }…

# Somebody who lost access could read them
t19bcc0de = Alguém que perdeu o acesso podia lê-los

# Manage access to {name}…
t56bf0afe = Gerenciar o acesso a { $name }…

# Who may connect, edit or manage it
t63fb49f5 = Quem pode se conectar, editá-la ou gerenciá-la

# Give the key of {name}…
t57ffca8e = Dar a chave de { $name }…

# To the people granted it who wait for it
tee158d1d = Às pessoas autorizadas que a aguardam

# Sync passwords with a folder…
t74de6b57 = Sincronizar senhas com uma pasta…

# Sync passwords with a server…
tc73a9bf6 = Sincronizar senhas com um servidor…

# Settings…
t64d0d0b4 = Configurações…

# Restart to install shellwing {version}
taadea299 = Reiniciar para instalar o shellwing { $version }

# shellwing {version} is out…
tab699ac8 = shellwing { $version } está disponível…

# Check for updates
t70ad7706 = Procurar atualizações

# New group…
t2c0113fa = Novo grupo…

# Undo import from {app}
t971f8deb = Desfazer a importação do { $app }

# Edit host {name}…
te6b404e6 = Editar servidor { $name }…

# Edit hosts.toml
tbfa1762c = Editar hosts.toml

# Edit config.toml
tf108a14d = Editar config.toml

# No snippet for this host: add one in Settings › Snippets.
tdd2847bb = Nenhum trecho para este servidor: adicione um em Configurações › Trechos.

# No config file for this host: add one in Settings › Config files.
t4b38a37d = Nenhum arquivo de configuração para este servidor: adicione um em Configurações › Arquivos de configuração.

# No environment for this host: add one in Settings › Environments.
t1b95315b = Nenhum ambiente para este servidor: adicione um em Configurações › Ambientes.

# This host uses the theme `{t}` from hosts.toml; change it there.
td55512c8 = Este servidor usa o tema `{ $t }` de hosts.toml; altere-o lá.

# Synced {n} change from other devices. | Synced {n} changes from other devices.
tffe9e6a0 =
    { $n ->
        [one] { $n } alteração sincronizada de outros dispositivos.
       *[other] { $n } alterações sincronizadas de outros dispositivos.
    }

# A note was changed on two devices: the other version is kept as {title}.
t9024ea9c = Uma nota foi alterada em dois dispositivos: a outra versão foi guardada como { $title }.

# {n} note was changed on two devices: the other version is kept beside it. | {n} notes were changed on two devices: the other versions are kept beside them.
tbe970732 =
    { $n ->
        [one] { $n } nota foi alterada em dois dispositivos: a outra versão foi guardada ao lado.
       *[other] { $n } notas foram alteradas em dois dispositivos: as outras versões foram guardadas ao lado.
    }

# Somebody lost access to {name}: its key is replaced. Change what they could read: Secrets to change in {name}, in the palette.
t6b22b8e8 = Alguém perdeu o acesso a { $name }: a chave da coleção foi substituída. Troque o que essa pessoa podia ler: Segredos a trocar em { $name }, na paleta.

# Somebody lost access to {name}: its key is replaced.
t189f91dc = Alguém perdeu o acesso a { $name }: a chave da coleção foi substituída.

# Sync: {e}
t8cb8c8c3 = Sincronização: { $e }

# The other device refused to let this one in.
t1d4ed983 = O outro dispositivo recusou a entrada deste.

# No device let this one in within 10 minutes. Ask again, or use the recovery key.
t410f845d = Nenhum dispositivo permitiu a entrada deste em 10 minutos. Peça de novo ou use a chave de recuperação.

# Pairing: {e}
te7df26f8 = Pareamento: { $e }

# Let {device} into this vault?
tfab743ab = Permitir a entrada de { $device } neste cofre?

# It gets everything the vault holds: passwords, keys, snippets and hosts. Approve only if {device} shows the code {code}.
tc6029096 = Ele recebe tudo o que o cofre guarda: senhas, chaves, trechos e servidores. Aprove somente se { $device } mostrar o código { $code }.

# Approve
te6edf55a = Aprovar

# The vault is locked
t78b7a6ee = O cofre está bloqueado

# Its passphrase opens it, or its recovery key (13 groups of four). Esc leaves it locked: Unlock the vault, in the palette, asks again.
t5eb207c8 = A frase secreta o abre, ou a chave de recuperação (13 grupos de quatro). Esc o mantém bloqueado: Desbloquear o cofre, na paleta, pergunta de novo.

# The system keychain lost the vault's key
t444b9525 = O chaveiro do sistema perdeu a chave do cofre

# Its recovery key (13 groups of four) opens it, and puts the key back in the keychain.
ta628efeb = A chave de recuperação (13 grupos de quatro) o abre e devolve a chave ao chaveiro.

# {name} is asking
ta753cd6b = { $name } está pedindo

# Or type the passphrase here.
tb78be92d = Ou digite a frase secreta aqui.

# The vault's passphrase, or its recovery key
t81044cdb = A frase secreta do cofre ou a chave de recuperação

# The recovery key
tbaa8dc18 = A chave de recuperação

# The vault is open.
t3773d56a = O cofre está aberto.

# Not opened that way: {e}
t7693053c = Não foi aberto dessa forma: { $e }

# Type it again
ta4c19926 = Digite de novo

# To be sure of it.
te7d01782 = Para confirmar.

# There is no system keychain to keep the vault's key
t04970b55 = Não há chaveiro do sistema para guardar a chave do cofre

# A passphrase keeps it instead: you type it when shellwing starts. At least {min} characters. Esc: no saved passwords, keys or sync this time.
t2fd0f1b3 = Uma frase secreta a guarda no lugar dele: você a digita quando o shellwing inicia. Pelo menos { $min } caracteres. Esc: sem senhas salvas, chaves ou sincronização desta vez.

# A passphrase for the vault
t25841dd8 = Uma frase secreta para o cofre

# It opens the vault where the system keychain cannot. At least {min} characters.
tf7ebf632 = Ela abre o cofre onde o chaveiro do sistema não consegue. Pelo menos { $min } caracteres.

# A passphrase
tf2500b30 = Uma frase secreta

# That does not open it: {e}.
tdcd34ef8 = Isso não o abre: { $e }.

# The vault stays locked: {e}
t05ea3b39 = O cofre continua bloqueado: { $e }

# Too short: at least {min} characters.
tff7b3675 = Curta demais: pelo menos { $min } caracteres.

# The two were not the same.
t60aece29 = As duas não eram iguais.

# Recovery key
t02904a59 = Chave de recuperação

# Enter copies
t4cc7b1ae = Enter copia

# Write this key down and keep it safe.
t86925e9b = Anote esta chave e guarde-a em segurança.

# It opens the vault if the passphrase is forgotten; Settings › Vault shows it again.
t1de04861 = Ela abre o cofre se a frase secreta for esquecida; Configurações › Cofre a mostra de novo.

# The vault is ready; its passphrase keeps it.
t41eb3235 = O cofre está pronto; a frase secreta o protege.

# The passphrase opens the vault too now.
tb612b2c2 = Agora a frase secreta também abre o cofre.

# No passphrase set: {e}
t41c038bf = Nenhuma frase secreta definida: { $e }

# A passphrase that joins new devices
tde75a705 = Uma frase secreta que adiciona novos dispositivos

# Typed on a new device that syncs the same folder, instead of the recovery key. Anyone who can read the folder can try to guess it: four or more unrelated words, at least {min} characters.
t703339bb = Digitada em um novo dispositivo que sincroniza a mesma pasta, em vez da chave de recuperação. Qualquer pessoa que consiga ler a pasta pode tentar adivinhá-la: quatro ou mais palavras sem relação entre si, pelo menos { $min } caracteres.

# A sync passphrase
ta0a73805 = Uma frase secreta de sincronização

# {place} lets a passphrase join its vault
t2ac4c55f = { $place } permite entrar no cofre com uma frase secreta

# The one set on a device that syncs (Settings › Sync). Secrets saved here are kept and merged.
t14a40721 = A que foi definida em um dispositivo que sincroniza (Configurações › Sincronização). Os segredos salvos aqui são mantidos e mesclados.

# The sync passphrase
t9fb2f5f5 = A frase secreta de sincronização

# Too easy to guess (strength {strength} of 4; 3 is needed). Several unrelated words work well.
tb51562c6 = Fácil demais de adivinhar (força { $strength } de 4; é preciso 3). Várias palavras sem relação entre si funcionam bem.

# Not set: this device does not sync.
t3aadb1e5 = Não definida: este dispositivo não sincroniza.

# Setting the sync passphrase…
tbf6d3ccf = Definindo a frase secreta de sincronização…

# New devices can join with the sync passphrase now.
t2bd178bc = Novos dispositivos já podem entrar com a frase secreta de sincronização.

# Not set: {e}
t8229a0e7 = Não definida: { $e }

# Joining…
taa884c51 = Entrando…

# Cannot join: {e}
t6d9d6813 = Não é possível entrar: { $e }

# Approve it on a device that syncs
t70c052c8 = Aprovar em um dispositivo que sincroniza

# That device asks whether to let this one in; you check both show the same code.
tb533a5e4 = Esse dispositivo pergunta se deve permitir a entrada deste; você confere se os dois mostram o mesmo código.

# Type the recovery key
te9ca27fa = Digitar a chave de recuperação

# The 13 groups of four from when syncing started.
t23f96b77 = Os 13 grupos de quatro de quando a sincronização começou.

# Type the sync passphrase
t8cd53b17 = Digitar a frase secreta de sincronização

# The one a device that syncs set to let new devices join.
taf23bf2b = A que um dispositivo que sincroniza definiu para permitir a entrada de novos dispositivos.

# {folder} holds a vault from another device: join it
t3ef84906 = { $folder } contém um cofre de outro dispositivo: entrar nele

# Code {code}
t0e7c074b = Código { $code }

# {device} asks whether to let this device in. Approve there if it shows this same code.
t24c15ff5 = { $device } pergunta se deve permitir a entrada deste dispositivo. Aprove lá se ele mostrar este mesmo código.

# Waiting for a device that syncs
t6e41520f = Aguardando um dispositivo que sincroniza

# Open shellwing on a device that syncs this folder: it asks whether to let this one in. Esc cancels.
ted2b04ee = Abra o shellwing em um dispositivo que sincroniza esta pasta: ele pergunta se deve permitir a entrada deste. Esc cancela.

# Sync with this folder
t8eb0599a = Sincronizar com esta pasta

# it is still opening
tdc19bf2a = ele ainda está abrindo

# Cannot sync: the vault: {why}.
tad5847da = Não é possível sincronizar: o cofre: { $why }.

# Cannot keep the password: {e}
tcf7fa2ce = Não é possível guardar a senha: { $e }

# Connecting to {place}…
tea99befd = Conectando a { $place }…

# What the vault keeps there is encrypted, but anyone can see how many hosts it holds and when your devices sync. A private repository is safer.
t5376e59a = O que o cofre guarda lá é criptografado, mas qualquer pessoa pode ver quantos servidores ele contém e quando seus dispositivos sincronizam. Um repositório privado é mais seguro.

# Use another repository
tbbd1ade5 = Usar outro repositório

# Anyone can read this repository
t8df04abb = Qualquer pessoa pode ler este repositório

# Not connected: anyone can read this repository.
t649a5b7d = Não conectado: qualquer pessoa pode ler este repositório.

# Cannot sync: {e}
td3aecb42 = Não é possível sincronizar: { $e }

# The vault: {why}.
t775cecf8 = O cofre: { $why }.

# Joined: this device syncs with {place}.
t8992145b = Pronto: este dispositivo sincroniza com { $place }.

# Cannot save where the vault syncs: {e}
t7eec807c = Não é possível salvar onde o cofre sincroniza: { $e }

# {folder} holds a vault from another device.
tccddb8cb = { $folder } contém um cofre de outro dispositivo.

# Type its recovery key to share it. Secrets saved here are kept and merged.
t7915bff9 = Digite a chave de recuperação dele para compartilhá-lo. Os segredos salvos aqui são mantidos e mesclados.

# That recovery key does not open it.
t2418fa69 = Essa chave de recuperação não o abre.

# Check each group of four; letters and case do not need to be exact.
t6a984230 = Confira cada grupo de quatro; letras e maiúsculas/minúsculas não precisam ser exatas.

# Recovery key: XXXX-XXXX-…
t931d6f73 = Chave de recuperação: XXXX-XXXX-…

# Connected
t7bf69370 = Conectados

# Other hosts · connected first
t63758b2c = Outros servidores · conectados primeiro

# Install {key} on…
t80b2a794 = Instalar { $key } em…

# {key} goes on here once connected.
t02f786c3 = { $key } será instalada aqui assim que conectar.

# no config directory
t33e2c7cb = nenhum diretório de configuração

# {host} signs in with {key} from now on.
t054dfae0 = { $host } entra com { $key } daqui em diante.

# Cannot edit hosts.toml: {e}
t917924da = Não é possível editar hosts.toml: { $e }

# Saved {name}.
t966c9f6d = { $name } salvo.

# Collections need syncing with a shellwing server.
t2c569ab8 = As coleções precisam ser sincronizadas com um servidor shellwing.

# Cannot list who waits: {e}
t27666c65 = Não é possível listar quem aguarda: { $e }

# Fingerprint {fingerprint}: compare it with the one on their Profile page, then give.
te5720513 = Impressão digital { $fingerprint }: compare-a com a da página de Perfil da pessoa e depois dê a chave.

# Suspended in the organization: they can have it once they are back.
t52d41370 = Suspenso na organização: poderá recebê-la quando voltar.

# Has no encryption key yet: they sync the shellwing app with the server once.
t7c7319f3 = Ainda não tem chave de criptografia: a pessoa precisa sincronizar o app shellwing com o servidor uma vez.

# Everybody with access holds the key.
td9e661ee = Todos com acesso já têm a chave.

# Give the key of {name} to whom?
t2cdda213 = Dar a chave de { $name } a quem?

# Cannot read who has access: {e}
t5566362d = Não é possível ler quem tem acesso: { $e }

# {permission}, a group
t7a59c6dc = { $permission }, um grupo

# Grant somebody…
td007775d = Conceder acesso a alguém…

# A person or a group of the organization
tf1974207 = Uma pessoa ou um grupo da organização

# Who has access to {name}
t7a73df49 = Quem tem acesso a { $name }

# Owner: has Manage already
t430fba35 = Proprietário: já tem “Gerenciar”

# Suspended: has it once active again
td816223c = Suspenso: terá acesso quando estiver ativo de novo

# A group: everybody in it
t185fe798 = Um grupo: todos que fazem parte dele

# Everybody of the organization has access already.
t65fbe8e5 = Todos da organização já têm acesso.

# Grant {name} to whom?
tc1a460bb = Conceder acesso a { $name } a quem?

# {detail} · now
t9ca9aebd = { $detail } · atual

# Take access away
t095b309e = Remover o acesso

# The collection gets a new key, which they never have
tf16d52b6 = A coleção recebe uma nova chave, que essa pessoa nunca terá

# What may {who} do in {name}?
tcfce5912 = O que { $who } pode fazer em { $name }?

# {who} no longer has access to {name}.
tc39eb663 = { $who } não tem mais acesso a { $name }.

# {who} has {permission} on {name} now.
tf84e42da = { $who } agora tem “{ $permission }” em { $name }.

# {who} has {permission} on {name}. Give them its key once they wait for it: Give the key of {name}, in the palette.
t73393a50 = { $who } tem “{ $permission }” em { $name }. Dê a chave quando essa pessoa a aguardar: Dar a chave de { $name }, na paleta.

# Access was not changed: {e}
tdf3aeb2c = O acesso não foi alterado: { $e }

# All changed: take this list away
tc0346301 = Tudo trocado: remover esta lista

# Change each where it is used, then here in the collection.
t4e94b74c = Troque cada um onde é usado e depois aqui na coleção.

# Somebody who lost access to {name} could read these: change them
t4bd96d3f = Alguém que perdeu o acesso a { $name } podia ler estes: troque-os

# {name} has the key of {collection} now.
t4dd83f7b = { $name } agora tem a chave de { $collection }.

# The key was not given: {e}
t9260982f = A chave não foi dada: { $e }

# Saved the group {name}.
t58507cbd = Grupo { $name } salvo.

# Deleted the group {name}.
t2dbe70db = Grupo { $name } excluído.

# Imported {n} host from {app}. | Imported {n} hosts from {app}.
t57e0e370 =
    { $n ->
        [one] { $n } servidor importado do { $app }.
       *[other] { $n } servidores importados do { $app }.
    }

# {n} left out. | {n} left out.
t96ae7881 =
    { $n ->
        [one] { $n } deixado de fora.
       *[other] { $n } deixados de fora.
    }

# The palette can undo it.
t21bfdf84 = A paleta pode desfazer isso.

# The import from {app} is undone.
t5f37b9b7 = A importação do { $app } foi desfeita.

# Port 8080 here reaches localhost:80 as the server sees it.
t51796bc3 = A porta 8080 aqui alcança localhost:80 como o servidor o vê.

# Port 9000 on the server reaches localhost:3000 here.
t79226dc9 = A porta 9000 no servidor alcança localhost:3000 aqui.

# A SOCKS proxy on port 1080 here; connections go out from the server.
t8be5905e = Um proxy SOCKS na porta 1080 aqui; as conexões saem do servidor.

# For this tab, until it is closed
tfe049b9e = Para esta aba, até ela ser fechada

# To forward on every connection, add it to the host's forward list in hosts.toml.
te97c1f00 = Para redirecionar em toda conexão, adicione-o à lista de redirecionamentos do servidor em hosts.toml.

# Syncing. Write this key down and keep it safe.
t967ce868 = Sincronizando. Anote esta chave e guarde-a em segurança.

# The key of the synced vault.
te0a56fe7 = A chave do cofre sincronizado.

# Another device needs it to join; it also opens the vault if this device is lost.
ta32e11fd = Outro dispositivo precisa dela para entrar; ela também abre o cofre se este dispositivo for perdido.

# the vault is still opening
t0cf7277a = o cofre ainda está abrindo

# No saved passwords: {why}.
tf97d3647 = Sem senhas salvas: { $why }.

# Password for {target}
t2cd4c302 = Senha de { $target }

# Enter to forget
t7512451f = Enter para esquecer

# Passphrase of {key_path}
t328fcceb = Frase secreta de { $key_path }

# Cannot read the vault: {e}
taee646ba = Não é possível ler o cofre: { $e }

# Nothing saved yet: after a password works, the pane offers to save it.
t586cd916 = Nada salvo ainda: depois que uma senha funciona, o painel oferece salvá-la.

# Pairing cancelled.
t9bf01ace = Pareamento cancelado.

# Cannot take the list away: {e}
t65900621 = Não é possível remover a lista: { $e }

# Forgotten.
te8ff7bb9 = Esquecida.

# Theme: {name}
ted8446cd = Tema: { $name }

# Cannot read the key: {e}
t744465b3 = Não é possível ler a chave: { $e }

# the system keychain does not hold the vault's key: without a passphrase, only the recovery key would open it
td554dbfe = o chaveiro do sistema não guarda a chave do cofre: sem uma frase secreta, só a chave de recuperação o abriria

# The vault opens with the system keychain alone now.
t5426fdee = Agora o cofre abre só com o chaveiro do sistema.

# Passphrase kept: {e}.
tcc4fe4d2 = Frase secreta mantida: { $e }.

# Presence
t834965a4 = Presença

# {name} opens the vault now, as its passphrase does.
t30f109d1 = Agora { $name } abre o cofre, como a frase secreta.

# {name} no longer opens the vault.
t92f8fdac = { $name } não abre mais o cofre.

# The vault is locked.
tc91b8040 = O cofre está bloqueado.

# The vault cannot lock: the system keychain opens it, or it has no passphrase.
t2e6801fd = O cofre não pode ser bloqueado: o chaveiro do sistema o abre, ou ele não tem frase secreta.

# The vault's key is out of the system keychain: each start asks for the passphrase.
tc8dd0fb9 = A chave do cofre saiu do chaveiro do sistema: cada início pede a frase secreta.

# The key stays in the keychain: {e}.
tdbb24cfc = A chave continua no chaveiro: { $e }.

# New devices join with the recovery key, or by approving them.
t4b5eba19 = Novos dispositivos entram com a chave de recuperação ou sendo aprovados.

# Not stopped: {e}
t3ea0b584 = Não interrompido: { $e }

# The system keychain keeps the vault's key now.
tbdfd4327 = Agora o chaveiro do sistema guarda a chave do cofre.

# Stopped syncing. What is there and the secrets here are kept.
t4ba0ed42 = Sincronização interrompida. O que está lá e os segredos daqui são mantidos.

# Cannot create the file: {e}
te92446ce = Não é possível criar o arquivo: { $e }

# No config directory.
t16df9b1f = Nenhum diretório de configuração.

# The vault locked: {n} minute without use. | The vault locked: {n} minutes without use.
t9abc648a =
    { $n ->
        [one] O cofre foi bloqueado: { $n } minuto sem uso.
       *[other] O cofre foi bloqueado: { $n } minutos sem uso.
    }

# Passwords cannot be saved: {p}
t1ce3459e = Não é possível salvar senhas: { $p }

# this computer
t7d16e2ca = este computador

# Local terminal
t50537e20 = Terminal local

# Typing goes to {listening} of {n} pane. Click a pane's badge to leave it out or bring it back. | Typing goes to {listening} of {n} panes. Click a pane's badge to leave it out or bring it back.
t3023d2ae =
    { $n ->
        [one] A digitação vai para { $listening } de { $n } painel. Clique no selo de um painel para deixá-lo de fora ou trazê-lo de volta.
       *[other] A digitação vai para { $listening } de { $n } painéis. Clique no selo de um painel para deixá-lo de fora ou trazê-lo de volta.
    }

# Stop ({keys})
t6ae11828 = Parar ({ $keys })

# This computer
tfa0d176a = Este computador

# Favorites
t7c2c53ba = Favoritos

# read only
t0d7ec241 = somente leitura

# waiting for its key
tb827ed42 = aguardando a chave

# Shared · {org}
t280d0e57 = Compartilhados · { $org }

# Shared
teb0bbef4 = Compartilhados

# find a host, connect, run an action
t4b795631 = encontrar um servidor, conectar, executar uma ação

# a new tab
t8ed35e75 = uma nova aba

# close the tab
teb891a97 = fechar a aba

# next tab
ta7702de7 = próxima aba

# go to a tab
tf70ede40 = ir para uma aba

# show or hide the host list
t9489dcc2 = mostrar ou ocultar a lista de servidores

# Choose a host on the left to connect
t8354c717 = Escolha um servidor à esquerda para conectar

# or press {keys} to find one, or to connect to user@host
t4f67c373 = ou pressione { $keys } para encontrar um, ou para conectar a user@host

# Connects to its hosts; cannot change them
t316bd4ec = Conecta-se aos servidores dela; não pode alterá-los

# Changes its hosts
t8d9014d3 = Altera os servidores dela

# Manage
td7aff116 = Gerenciar

# Edits, grants access and gives the key
t7c52884e = Edita, concede acesso e dá a chave

# Shared in {org}: everybody granted it sees its hosts, end-to-end encrypted.
t2b0a441e = Compartilhada em { $org }: todos com acesso veem seus servidores, com criptografia de ponta a ponta.

# Shared in an organization: everybody granted it sees its hosts, end-to-end encrypted.
tb7742f20 = Compartilhada em uma organização: todos com acesso veem seus servidores, com criptografia de ponta a ponta.

# Runs a command on this computer to connect: {command}
tb315c6bd = Executa um comando neste computador para conectar: { $command }

# Goes through {hosts} first, as this computer knows them
t819b0018 = Passa primeiro por { $hosts }, como este computador os conhece

# Forwards ports: {forward}
tde2adb5a = Encaminha portas: { $forward }

# Lets the host use your ssh-agent while connected
t848bef88 = Deixa o servidor usar seu ssh-agent enquanto conectado

# Lets its programs open windows here and read your keyboard (trusted X11)
ted3d4c0e = Deixa os programas dele abrirem janelas aqui e lerem seu teclado (X11 confiável)

# Lets its programs open windows here (X11)
t7d003faa = Deixa os programas dele abrirem janelas aqui (X11)

# Takes the host's key the first time without asking you
tbda4f1d9 = Aceita a chave do servidor na primeira vez sem perguntar

# Types a command into its shell once connected: {command}
t0a40f947 = Digita um comando no shell dele ao conectar: { $command }

# Name: another host has this name.
tcf956656 = Nome: outro servidor tem este nome.

# Alias: another host uses this.
t6d4194b8 = Alias: outro servidor já usa este.

# The command is {size} KB; {max} KB at most.
ta78b9fbd = O comando tem { $size } KB; no máximo { $max } KB.

# {name}: a name takes {max} characters at most.
t190cce46 = { $name }: um nome tem no máximo { $max } caracteres.

# {name} is {size} KB; a value takes {max} KB at most.
t8a0064ca = { $name } tem { $size } KB; um valor tem no máximo { $max } KB.

# {count} variables; {max} at most.
t1c9f770c = { $count } variáveis; no máximo { $max }.

# The variables come to {size} KB; {max} KB at most.
tdbf76954 = As variáveis somam { $size } KB; no máximo { $max } KB.

# The file is {size} KB; {max} KB at most.
tf2bc7ef2 = O arquivo tem { $size } KB; no máximo { $max } KB.

# The note is {size} KB; {max} KB at most.
td5e1ea7c = A nota tem { $size } KB; no máximo { $max } KB.

# The image is {size} KB; {max} KB at most.
t7bb41ed9 = A imagem tem { $size } KB; no máximo { $max } KB.

# The note's images come to {size} MB; {max} MB at most.
t085c57f3 = As imagens da nota somam { $size } MB; no máximo { $max } MB.

# Not an image that can be kept: a PNG or a JPEG.
tccae79a4 = Não é uma imagem que possa ser guardada: um PNG ou um JPEG.

# The shell did not take the variables: it is not at its prompt, or not a POSIX shell (sh, bash, zsh).
ta1a9b215 = O shell não aceitou as variáveis: ele não está no prompt ou não é um shell POSIX (sh, bash, zsh).

# It has no variables.
t0993a534 = Ele não tem variáveis.

# With the ones loaded, that comes to {size} KB; a shell takes {max} KB at most: unload one first.
t3883bd87 = Com os já carregados, somam { $size } KB; um shell aceita no máximo { $max } KB: descarregue um antes.

# this device syncs with no shellwing server: connect one in Settings › Sync
t10561845 = este dispositivo não sincroniza com nenhum servidor shellwing: conecte um em Configurações › Sincronização

# this device is not signed in to the server: connect again in Settings › Sync
t0ce953a7 = este dispositivo não está conectado ao servidor: conecte de novo em Configurações › Sincronização

# every host
t444612de = todos os servidores

# its password is not saved on this device: connect again in Settings › Sync
td2fdfc60 = a senha não está salva neste dispositivo: conecte de novo em Configurações › Sincronização

# {title} (from {device})
t63a0d436 = { $title } (de { $device })

# {title} (from another device)
t0e55da75 = { $title } (de outro dispositivo)

# hosts not synced: {e}
te7d5212e = servidores não sincronizados: { $e }

# settings not synced: {e}
tf8592ea2 = configurações não sincronizadas: { $e }

# settings not written: {e}
t60f438b7 = configurações não gravadas: { $e }

# hosts not written: {e}
t00f3a4eb = servidores não gravados: { $e }

# Where it goes: a file's path from / or from ~/, such as ~/.kube/config.
t37ca034c = Onde ele vai: o caminho de um arquivo a partir de / ou de ~/, como ~/.kube/config.

# Nothing is at {path}.
ta5cf42ea = Não há nada em { $path }.

# They do not wait for its key any more.
tf982de37 = Essa pessoa não aguarda mais a chave.

# Address: an address is needed.
t3f9c8dd7 = Endereço: é preciso um endereço.

# Address: no spaces.
t1a9d2c69 = Endereço: sem espaços.

# Alias: no spaces.
t240468fd = Alias: sem espaços.

# Port: 1 to 65535.
t48e7bff2 = Porta: de 1 a 65535.

# User: no spaces.
t7b146164 = Usuário: sem espaços.

# Key: choose a key.
t96503d0f = Chave: escolha uma chave.

# Keepalive: seconds, 0 to 3600 (0: none).
t868fa0cd = Keepalive: segundos, de 0 a 3600 (0: nenhum).

# Jump: a host cannot jump through itself.
tf5b576a7 = Salto: um servidor não pode saltar por si mesmo.

# Forward {forward}: {e}
t575e8c34 = Redirecionamento { $forward }: { $e }

# the password for {target}
t33a923dd = a senha de { $target }

# the passphrase of {path}
t847f4536 = a frase secreta de { $path }

# A full-screen program is running: leave it, then try again.
t1dca493f = Um programa em tela cheia está rodando: saia dele e tente de novo.

# Unlock shellwing
t25d6e4bc = Desbloquear o shellwing

# {biometry} or your passcode unlocks it.
t22b7de26 = { $biometry } ou o seu código o desbloqueia.

# Your passcode unlocks it.
t22c93782 = O seu código o desbloqueia.

# {n} session stays connected underneath. | {n} sessions stay connected underneath.
t8f2f0709 =
    { $n ->
        [one] { $n } sessão continua conectada por baixo.
       *[other] { $n } sessões continuam conectadas por baixo.
    }

# No config files yet: files kept in the vault (a kubeconfig, an .npmrc), placed on a host from a session.
t773e80be = Nenhum arquivo de configuração ainda: arquivos guardados no cofre (um kubeconfig, um .npmrc), colocados em um servidor a partir de uma sessão.

# stays
t6210bca9 = fica

# Placed to stay
t326d488a = Colocado para ficar

# Placed, it stays when the session ends.
t596dc3ba = Depois de colocado, ele fica quando a sessão termina.

# Placed, it is taken away when the session ends.
tcb169849 = Depois de colocado, ele é retirado quando a sessão termina.

# Binary, {size}: kept as it is.
td3506722 = Binário, { $size }: mantido como está.

# Replace from Files…
t910b03ca = Substituir a partir de Arquivos…

# Only a text file can be imported here.
t94955dee = Só um arquivo de texto pode ser importado aqui.

# Permissions: three digits from 0 to 7, such as 600.
tad710134 = Permissões: três dígitos de 0 a 7, por exemplo 600.

# No config files for this host. Keep them in Library › Files.
t31aa06de = Nenhum arquivo de configuração para este servidor. Guarde-os em Biblioteca › Arquivos.

# Place a config file
t2bd139bb = Colocar um arquivo de configuração

# Replace the one placed until the session ends
t72427900 = Substituir o colocado até a sessão terminar

# Place it until the session ends
tf442be28 = Colocar até a sessão terminar

# Replace it until the session ends
t96406163 = Substituir até a sessão terminar

# Cannot place {name}: {error}
t816e11b0 = Não foi possível colocar { $name }: { $error }

# {name} is at {path} until the session ends.
t10182ed5 = { $name } está em { $path } até a sessão terminar.

# {name} is at {path}.
t3b2e2230 = { $name } está em { $path }.

# If the vault syncs, it goes from your other devices too.
t3c800198 = Se o cofre for sincronizado, ele some também dos seus outros dispositivos.

# The folder and everything in it are deleted on {host}.
t200fdede = A pasta e tudo o que há nela são excluídos em { $host }.

# It is deleted on {host}.
td57ed776 = Ele é excluído em { $host }.

# Upload from Files…
t3efaa7bd = Enviar de Arquivos…

# Hidden files
t3f0de492 = Arquivos ocultos

# Add
t9dc3aa14 = Adicionar

# Make
t486bedcf = Criar

# Download the folder
t1c6b8ca2 = Baixar a pasta

# Keep in the vault
t08dcb490 = Guardar no cofre

# Copy the path
t095d064e = Copiar o caminho

# A name without /, please.
t7055c52a = Um nome sem /, por favor.

# Making {name}…
t431597d3 = Criando { $name }…

# Renaming…
t87125000 = Renomeando…

# Deleting {name}…
ta17d02e6 = Excluindo { $name }…

# {name} deleted.
tf36570a5 = { $name } excluído.

# Downloading {name}…
t49917320 = Baixando { $name }…

# Opening {name}…
t24b03450 = Abrindo { $name }…

# Uploading {name}…
t763593ef = Enviando { $name }…

# Uploading {n} item… | Uploading {n} items…
t4d21b20c =
    { $n ->
        [one] Enviando { $n } item…
       *[other] Enviando { $n } itens…
    }

# {name} uploaded.
t0b81da1e = { $name } enviado.

# {n} item uploaded. | {n} items uploaded.
tbf7cc840 =
    { $n ->
        [one] { $n } item enviado.
       *[other] { $n } itens enviados.
    }

# Keeping {name}…
tbb0f5ffb = Guardando { $name }…

# {name} is in the vault: Library › Files.
t71be4d4c = { $name } está no cofre: Biblioteca › Arquivos.

# {path} changed on {host} since it was opened.
ta4924204 = { $path } foi alterado em { $host } desde que foi aberto.

# Replace it with this
tf329d9b0 = Substituir por este

# Not saved: {error}
t2fe29775 = Não salvo: { $error }

# A host's own theme wins over its group's. Choose the group in a host's editor to put the host in it.
t3ff468c2 = O tema próprio de um servidor prevalece sobre o do grupo. Escolha o grupo no editor de um servidor para colocar o servidor nele.

# Delete {group}?
t6801beba = Excluir { $group }?

# Delete {group}? Its {n} host stays, in no group. | Delete {group}? Its {n} hosts stay, in no group.
t8d5eff39 =
    { $n ->
        [one] Excluir { $group }? Seu { $n } servidor permanece, sem grupo.
       *[other] Excluir { $group }? Seus { $n } servidores permanecem, sem grupo.
    }

# No color
te96206b1 = Sem cor

# as the server says
ta204abe7 = como o servidor disser

# Alias
t22e1f2f7 = Alias

# Make a key for this host
tfc47113a = Gerar uma chave para este servidor

# Make a key for {host}
tc4091336 = Gerar uma chave para { $host }

# {name} is new: the host does not have it yet. Connect once another way (a password), then install it from the session's menu.
t14c6a565 = { $name } é nova: o servidor ainda não a tem. Conecte uma vez de outra forma (com uma senha) e depois instale-a pelo menu da sessão.

# Tries the keys in the vault, then asks for a password, like ssh.
t4c5dd8b2 = Tenta as chaves do cofre, depois pede uma senha, como o ssh.

# A key file of another device: this phone signs in with the vault's keys.
t85abc601 = Um arquivo de chave de outro dispositivo: este celular entra com as chaves do cofre.

# Uses the keys in an ssh-agent: a desktop's; a phone has none.
t4b96b415 = Usa as chaves de um ssh-agent: o de um computador; um celular não tem.

# Add a jump
t16c79e2d = Adicionar um salto

# Straight to the host. A jump goes through another host first (a bastion).
t31e98c98 = Direto ao servidor. Um salto passa primeiro por outro servidor (um bastion).

# Favorite: listed first
t7bd2a9c8 = Favorito: listado primeiro

# Keep the shell on the host (tmux)
ta3e2fb16 = Manter o shell no servidor (tmux)

# In tmux, a connection made again finds the same shell, its programs still running: the phone may lose it in the background. tmux has to be on the host; without it, the shell is as usual.
t6b4d9cba = Com tmux, uma conexão refeita encontra o mesmo shell, com os programas ainda rodando: o telefone pode perder a conexão em segundo plano. O tmux precisa estar no servidor; sem ele, o shell fica como sempre.

# Host keys
t2f5a3205 = Chaves de servidor

# As in Settings
t46210079 = Como em Configurações

# Accept new ones
tc99d0b88 = Aceitar as novas

# Known ones only
t2234971d = Só as conhecidas

# Keepalive (s)
t13e524f5 = Keepalive (s)

# as in Settings
tf8e77e19 = como em Configurações

# Another session
t4d6f66b8 = Outra sessão

# Edit {name}
t4688b082 = Editar { $name }

# Waiting for somebody who manages it to give you its key.
t42b71640 = Aguardando alguém que gerencia a coleção dar a chave a você.

# Add a host
t38d39c8f = Adicionar um servidor

# A collection has a new key
t0dc9c1a6 = Uma coleção tem uma nova chave

# Somebody lost access to {name}: its key is replaced. Change what they could read: they are listed above your hosts.
t4672bd05 = Alguém perdeu o acesso a { $name }: a chave da coleção foi substituída. Troque o que essa pessoa podia ler: a lista fica acima dos seus servidores.

# Add one with +, or sync the hosts of your other devices.
tceb03600 = Adicione um com + ou sincronize os servidores dos seus outros dispositivos.

# Give
t1978793c = Dar

# Give the key to {name}?
t51bb693b = Dar a chave a { $name }?

# Permission
t9bc44a22 = Permissão

# Access saved. Anybody newly granted waits for the key: give it from the key button.
tffdfd275 = Acesso salvo. Quem acabou de receber acesso aguarda a chave: dê a chave pelo botão da chave.

# Access saved.
t59c55f0a = Acesso salvo.

# the Keychain said {status}
tc1738579 = o chaveiro respondeu { $status }

# Envs
tf645a165 = Ambientes

# Part
te3596034 = Seção

# Library
ta8532270 = Biblioteca

# No snippets yet. Make one with +, or sync those of your other devices.
tf81b77f9 = Nenhum trecho ainda. Crie um com + ou sincronize os dos seus outros dispositivos.

# No snippets for this host.
tbf3767c5 = Nenhum trecho para este servidor.

# Browse the snippet library
t63787b2e = Explorar a biblioteca de trechos

# Filter snippets
tbb9b10c7 = Filtrar trechos

# Offered
t2bd643c4 = Oferecido

# Everywhere
t327a596f = Em todo lugar

# Group: {group}
t0d904ce9 = Grupo: { $group }

# Host: {host}
t3fb3f6b3 = Servidor: { $host }

# Press Enter after it
te4ce8331 = Pressionar Enter depois

# Command
tc67c8f52 = Comando

# {{name}} asks for a value when it runs; {{host}}, {{user}} and {{address}} come from the session.
td02256f4 = {"{{"}name{"}}"} pede um valor quando é executado; {"{{"}host{"}}"}, {"{{"}user{"}}"} e {"{{"}address{"}}"} vêm da sessão.

# Runs with
te90c2175 = Executa com

# Their variables are given to it alone, in a subshell, without showing their values.
t4ef64333 = As variáveis deles são passadas só para ele, em um subshell, sem mostrar os valores.

# No environments yet: variables kept in the vault, loaded into a session's shell without showing their values.
tbcdba796 = Nenhum ambiente ainda: variáveis guardadas no cofre, carregadas no shell de uma sessão sem mostrar os valores.

# Hide values
tab6d76ed = Ocultar valores

# Show values
t68f568ba = Mostrar valores

# NAME=value, a line. A name with nothing after = keeps the value it has.
tae47acf2 = NAME=value, um por linha. Um nome sem nada depois de = mantém o valor que tem.

# Values shown hide again after a minute, or when you leave the app.
t9608e14a = Os valores mostrados voltam a se ocultar depois de um minuto, ou quando você sai do app.

# Not shown, of several lines: {names}. The desktop shows them.
tf0ef55de = Não mostrados, por terem várias linhas: { $names }. O app de desktop os mostra.

# Show the values of {name}
t02678451 = Mostrar os valores de { $name }

# No notes yet.
t4be168d5 = Nenhuma nota ainda.

# Untitled
t2e35a678 = Sem título

# this host
t4b4cbe3d = este servidor

# Delete this note?
t9d3e3253 = Excluir esta nota?

# Filter notes
tde67183a = Filtrar notas

# Markdown
tf651116c = Markdown

# Show a sensitive note
t905684dd = Mostrar uma nota confidencial

# For hosts…
t4d6ecc3d = Para servidores…

# Insert an image…
t2d3653b2 = Inserir uma imagem…

# Note
t1440c15d = Nota

# That image cannot be read.
tc5fceb9b = Não é possível ler essa imagem.

# For hosts
t350fa483 = Para servidores

# Bold
t45768d96 = Negrito

# Italic
t8db0c08d = Itálico

# Strikethrough
t2504f1e4 = Tachado

# Code
t795db914 = Código

# Link
t8e08e589 = Link

# Heading
tfc3003b5 = Título

# Table
td70ceb7f = Tabela

# Outdent
ta6a77b16 = Diminuir recuo

# Indent
t5f8d60db = Aumentar recuo

# Undo
t71fd4acf = Desfazer

# Redo
t7412e5e9 = Refazer

# Hide the keyboard
t5cadc433 = Ocultar o teclado

# Not done
t0f88a146 = Não feito

# Image
t590ca79a = Imagem

# Copied
t8dc21305 = Copiada

# Tip
t1d97162a = Dica

# Important
tc93c9dc5 = Importante

# Warning
t37c4fc8f = Aviso

# Caution
tec06ed06 = Cuidado

# No sessions open
t483ac645 = Nenhuma sessão aberta

# Open one from Hosts.
t7a72807e = Abra uma em Servidores.

# Sessions
tb69a8aec = Sessões

# Connect again
t05dc6981 = Conectar de novo

# {n} minute | {n} minutes
tf77c9aaa =
    { $n ->
        [one] { $n } minuto
       *[other] { $n } minutos
    }

# Lock
t71b42d62 = Bloqueio

# When shellwing starts, and after this long in the background, it hides until {biometry} or your passcode says it is you. A light guard for a phone left unlocked: the sessions go on underneath.
taaa2c58c = Quando o shellwing inicia, e após esse tempo em segundo plano, ele fica oculto até que { $biometry } ou o seu código confirme que é você. Uma proteção leve para um celular deixado desbloqueado: as sessões continuam por baixo.

# When shellwing starts, and after this long in the background, it hides until your passcode says it is you. A light guard for a phone left unlocked: the sessions go on underneath.
t5b7bea20 = Quando o shellwing inicia, e após esse tempo em segundo plano, ele fica oculto até que o seu código confirme que é você. Uma proteção leve para um celular deixado desbloqueado: as sessões continuam por baixo.

# This phone has no passcode, so nothing could ask that it is you.
td366e372 = Este celular não tem código, então nada poderia confirmar que é você.

# As the phone
t8ad2f046 = Igual ao celular

# Text size
t2c041143 = Tamanho do texto

# History
t7d96597f = Histórico

# Sessions opened after a change take it. A pinch on the terminal sizes its text too; the size is this phone's own.
tde3581cd = As sessões abertas após uma alteração passam a usá-lo. Um gesto de pinça no terminal também ajusta o tamanho do texto; o tamanho é próprio deste celular.

# New host keys
t48a98987 = Novas chaves de servidor

# Accept them
t2dd33559 = Aceitá-las

# Refuse them
t2483aa25 = Recusá-las

# Every {n} second | Every {n} seconds
t0877f108 =
    { $n ->
        [one] A cada { $n } segundo
       *[other] A cada { $n } segundos
    }

# Connections
t25db363e = Conexões

# A host's own, in its editor, wins. A key that changed is always refused: forget the host in Known hosts to accept its new one.
t25360db5 = A configuração própria de um servidor, no editor dele, prevalece. Uma chave alterada é sempre recusada: esqueça o servidor em Servidores conhecidos para aceitar a nova.

# iOS stops an app soon after it leaves the screen, and its connections with it. Reconnect makes a lost one again by itself, waiting longer each time, and at once when you come back; a host's shell kept in tmux (in its editor) is found as it was.
t8012e2b7 = O iOS para um app logo depois que ele sai da tela, e as conexões dele junto. “Reconectar” refaz sozinho uma conexão perdida, esperando mais a cada vez, e na hora em que você volta; o shell de um servidor mantido no tmux (no editor do servidor) é encontrado como estava.

# SSH keys
t7e7cad2b = Chaves SSH

# Saved passwords
tcdae5fca = Senhas salvas

# The vault is opening…
t12986dd4 = O cofre está abrindo…

# Passwords and keys, encrypted on this phone; its key is in the iOS Keychain, on this phone only.
t8cae68cf = Senhas e chaves, criptografadas neste celular; a chave do cofre fica no chaveiro do iOS, somente neste celular.

# Configuration
t1a5ee459 = Configuração

# hosts.toml, in the app's own folder
tcdaeae19 = hosts.toml, na pasta do próprio app

# Version
t5dcdd537 = Versão

# shellwing on GitHub
t1fcb667e = shellwing no GitHub

# Problem
t448c5c7c = Problema

# No host keys known yet: a host's is kept the first time you connect.
ta5bda1d5 = Nenhuma chave de servidor conhecida ainda: a de um servidor é guardada na primeira vez que você conecta.

# A hashed name
t97c138ac = Um nome com hash

# Forget
t6e3fab8c = Esquecer

# Forget the key of {host}?
t25034152 = Esquecer a chave de { $host }?

# It is shown to check again the next time you connect.
t51c13890 = Ela será mostrada para conferir na próxima vez que você se conectar.

# Filter by name
tbcb12ca7 = Filtrar por nome

# {n} snippet added: run it from a session's Snippets key. | {n} snippets added: run them from a session's Snippets key.
tcc9793a7 =
    { $n ->
        [one] { $n } trecho adicionado: execute-o pela tecla Trechos de uma sessão.
       *[other] { $n } trechos adicionados: execute-os pela tecla Trechos de uma sessão.
    }

# Asked when it runs: {names}. Once added, it is yours to edit.
t2b67b8d3 = Perguntado ao executar: { $names }. Depois de adicionado, ele é seu e você pode editá-lo.

# None yet: add one in Library › Envs.
te4d214e4 = Nenhum ainda: adicione um em Biblioteca › Ambientes.

# Picked
t71962a99 = Marcado

# shellwing may not use that folder.
t39f14b58 = O shellwing não tem permissão para usar essa pasta.

# This phone syncs there now. The recovery key opens the vault on your other devices: write it down.
t237ca793 = Este celular agora sincroniza lá. A chave de recuperação abre o cofre nos seus outros dispositivos: anote-a.

# Place
t9ad2489c = Local

# Last synced
t2d4f851d = Última sincronização

# not yet
t6d9d33dc = ainda não

# End-to-end encrypted: the place holds only what the vault's key opens. It syncs every minute while shellwing is open, and when something changes.
tac696b8b = Criptografado de ponta a ponta: o local guarda só o que a chave do cofre abre. Ele sincroniza a cada minuto enquanto o shellwing está aberto, e quando algo muda.

# {device} asks to join
tf44b389c = { $device } pede para entrar

# Approve only if it shows {code}. It gets everything the vault holds: passwords, keys, snippets and hosts.
t329319cf = Aprove somente se ele mostrar { $code }. Ele recebe tudo o que o cofre guarda: senhas, chaves, trechos e servidores.

# Devices asking
t49658daf = Dispositivos pedindo para entrar

# Stop syncing on this phone?
t7ca0b094 = Parar de sincronizar neste celular?

# The vault stays here, and the copy at the place stays for your other devices.
t7b47ff6d = O cofre continua aqui, e a cópia no local continua lá para os seus outros dispositivos.

# Waiting for a device that syncs there…
tb119de8b = Aguardando um dispositivo que sincroniza lá…

# Open shellwing on it: it asks whether to let this phone in.
t5b4d0d2f = Abra o shellwing nele: ele pergunta se deve permitir a entrada deste celular.

# {device} answered
t89d183da = { $device } respondeu

# Approve on {device} if it shows the same code.
tbc9bc44d = Aprove em { $device } se ele mostrar o mesmo código.

# Joined: this phone syncs there now.
tfb82a94e = Pronto: este celular agora sincroniza lá.

# Where
t91dad0f8 = Onde

# Password or app password
t9cc53273 = Senha ou senha de app

# Endpoint: https://s3.eu-west-1.amazonaws.com
t9deef30d = Endpoint: https://s3.eu-west-1.amazonaws.com

# Region (empty: auto)
t6689bdf6 = Região (vazio: automática)

# Folder in the bucket (optional)
t7c04f452 = Pasta no bucket (opcional)

# Token
t3a355bd2 = Token

# Choose another folder…
tcba15eff = Escolher outra pasta…

# Sync the vault
tff5adbd9 = Sincronizar o cofre

# Another device's vault is there. Join it to get its hosts, keys, passwords and snippets on this phone.
tfd5abd9b = O cofre de outro dispositivo está lá. Entre nele para ter os servidores, chaves, senhas e trechos dele neste celular.

# Pair with a device that syncs there
tb3c83579 = Parear com um dispositivo que sincroniza lá

# Join
t30025399 = Entrar

# XXXX-XXXX-…, 13 groups of 4
t1d59283c = XXXX-XXXX-…, 13 grupos de 4

# Join with the recovery key
t20a260d4 = Entrar com a chave de recuperação

# The passphrase set on the other device
t71d27652 = A frase secreta definida no outro dispositivo

# Join with the passphrase
t97bef16c = Entrar com a frase secreta

# A folder another device syncs in too, such as shellwing's in iCloud Drive.
t679b365b = Uma pasta que outro dispositivo também sincroniza, como a do shellwing no iCloud Drive.

# A private repository on GitHub, GitLab, Gitea or your own server, by its https:// address, with a token that may push there (on GitHub, a fine-grained token with Contents: read and write). Each change is a commit. The token is kept in this phone's vault.
t9526bd07 = Um repositório privado no GitHub, GitLab, Gitea ou no seu próprio servidor, pelo endereço https://, com um token que possa fazer push nele (no GitHub, um token de acesso refinado com Contents: leitura e escrita). Cada alteração é um commit. O token fica no cofre deste celular.

# The password is kept in this phone's vault, never in the place.
t76c422df = A senha fica no cofre deste celular, nunca no local.

# shellwing did not start.
td364ab30 = O shellwing não iniciou.

# Column {n}
tb3578e13 = Coluna { $n }

# Row {n}
tf0d5ffc7 = Linha { $n }

# Load an environment
tfd1fc961 = Carregar um ambiente

# Keep it there
t3cd7dd37 = Deixar lá

# Take it away now
te025e6eb = Retirar agora

# {name}, placed until the session ends
t7fad2b94 = { $name }, colocado até a sessão terminar

# Unload {name}
t4f418d0b = Descarregar { $name }

# {path} stays when the session ends.
t3ef97d99 = { $path } fica quando a sessão terminar.

# {path} taken away.
tb65f4b23 = { $path } retirado.

# Connected without what these hosts of an organization's collection ask. Whoever may edit the collection sets it, and it acts on this device: allow it only if you trust them.
t5e57676a = Conectado sem o que estes servidores da coleção de uma organização pedem. Quem pode editar a coleção define isso, e age neste dispositivo: permita só se confiar nessas pessoas.

# Save {offer} in the vault? It is tried first next time.
t3fd5ca20 = Salvar { $offer } no cofre? Na próxima vez, ela é tentada primeiro.

# Files (SFTP)…
tf423ea3c = Arquivos (SFTP)…

# Load an environment…
t8b267127 = Carregar um ambiente…

# Notes…
te20662aa = Notas…

# Place a config file…
tdc9941e5 = Colocar um arquivo de configuração…

# Install a key on this host…
te5f3e2ad = Instalar uma chave neste servidor…

# Left
t92773890 = Esquerda

# Up
t42f35940 = Cima

# Down
ta4995035 = Baixo

# Right
t1e9e9f85 = Direita

# Show the keyboard
t6884a1b4 = Mostrar o teclado

# Keys in the vault
t9c92000f = Chaves no cofre

# Make an Ed25519 key and install it
t1913ae2f = Gerar uma chave Ed25519 e instalá-la

# A new key
t93b96abd = Uma nova chave

# Kept in the vault, encrypted; only its public line goes to {host}.
teb44b03c = Guardada no cofre, criptografada; só a linha pública dela vai para { $host }.

# Install a key
td8f1fd20 = Instalar uma chave

# {name} is on {host}: it signs in from now on.
tcc1f38f4 = { $name } está em { $host }: daqui em diante, ela é usada para entrar.

# {name} was on {host} already.
t1b42e91e = { $name } já estava em { $host }.

# No keys yet. Make one, then install it on a host from a session's menu: it signs in from then on, without a password.
td823bccd = Nenhuma chave ainda. Gere uma e depois instale-a em um servidor pelo menu de uma sessão: a partir daí, ela é usada para entrar, sem senha.

# Share
t4f856dd8 = Compartilhar

# Hosts that sign in with it need another way in.
te6d5f68b = Os servidores que entram com ela vão precisar de outra forma de acesso.

# Make a key
ta2572fcb = Gerar uma chave

# A new Ed25519 key
t97812638 = Uma nova chave Ed25519

# Kept in the vault, encrypted, with your other secrets.
t698b5927 = Guardada no cofre, criptografada, com seus outros segredos.

# Nothing saved yet. When a password typed in a session works, shellwing offers to keep it.
t8e21fd8c = Nada salvo ainda. Quando uma senha digitada em uma sessão funciona, o shellwing oferece guardá-la.

# A key's passphrase
tdf0a144f = A frase secreta de uma chave

# Forget {name}?
t5b101fa2 = Esquecer { $name }?

# It is asked for again next time.
t736fd9ad = Será pedido de novo na próxima vez.

# Write it down and keep it apart from this phone. It opens the vault where nothing else can.
t91c53986 = Anote-a e guarde-a longe deste celular. Ela abre o cofre quando nada mais consegue.

# Show the vault's recovery key
t45dffa07 = Mostrar a chave de recuperação do cofre

# Not confirmed: the key stays hidden.
t899a4de3 = Não confirmado: a chave continua oculta.

# {n} session open | {n} sessions open
t113905d0 =
    { $n ->
        [one] { $n } sessão aberta
       *[other] { $n } sessões abertas
    }

# Close all
td20a2320 = Fechar todas

# Your fingerprint, face or screen lock unlocks it.
t218f9703 = Sua digital, seu rosto ou o bloqueio de tela desbloqueia.

# Keep sessions connected
t0bc12e52 = Manter as sessões conectadas

# While a session is open, a notification shows it and Android keeps it connected.
t1bfce276 = Enquanto houver uma sessão aberta, uma notificação a mostra e o Android a mantém conectada.

# Battery use
taa2fc90d = Uso da bateria

# Unrestricted: Android leaves the sessions alone.
tbc0972ef = Sem restrições: o Android deixa as sessões em paz.

# Optimized: Android may close the sessions in the background. Tap to allow unrestricted use.
tfabc103e = Otimizado: o Android pode fechar as sessões em segundo plano. Toque para permitir o uso sem restrições.
