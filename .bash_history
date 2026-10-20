 stty -echo; printf '\033]777;shellwing;r1\007'; __sw_v=; while IFS= read -r __sw_l && [ "$__sw_l" != . ]; do __sw_v=$__sw_v${__sw_l#>}; done; A=$(printf '%bX' "$__sw_v"); A=${A%X}; __sw_v=; while IFS= read -r __sw_l && [ "$__sw_l" != . ]; do __sw_v=$__sw_v${__sw_l#>}; done; B=$(printf '%bX' "$__sw_v"); B=${B%X}; __sw_v=; while IFS= read -r __sw_l && [ "$__sw_l" != . ]; do __sw_v=$__sw_v${__sw_l#>}; done; C=$(printf '%bX' "$__sw_v"); C=${C%X}; stty echo; unset __sw_v __sw_l; export A B C; printf '\033]777;shellwing;d1\007'
printf %s "$A" | od -v -An -tx1 | tr -d ' \n'; echo
printf %s "$B" | od -v -An -tx1 | tr -d ' \n'; echo
printf %s "$C" | od -v -An -tx1 | tr -d ' \n'; echo
exit
