#!/bin/bash

curl -L "https://www.unicode.org/Public/UCD/latest/ucd/Blocks.txt" -o ./unicode_file/Blocks.txt
curl -L "https://www.unicode.org/Public/UCD/latest/ucd/EastAsianWidth.txt" -o ./unicode_file/EastAsianWidth.txt
curl -L "https://www.unicode.org/Public/UCD/latest/ucd/Scripts.txt" -o ./unicode_file/Scripts.txt
