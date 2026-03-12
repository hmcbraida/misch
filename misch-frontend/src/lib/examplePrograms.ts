export type ExampleProgramId = 'echo' | 'primes' | 'vigenere';

export type ExampleProgram = {
	id: ExampleProgramId;
	label: string;
	assembly: string;
	paperTapeInput: string;
};

export const EXAMPLE_PROGRAMS: ExampleProgram[] = [
	{
		id: 'echo',
		label: 'Echo',
		assembly: `IN 2000(16)
OUT 2000(18)
HLT
END 0`,
		paperTapeInput: 'HELLO'
	},
	{
		id: 'primes',
		label: 'Primes',
		assembly: `* Prime printer with double output buffers.
* Reads one 5-char decimal word from paper tape (unit 16),
* prints that many primes to the line printer (unit 18).

PTAPE   EQU 16
PRINTER EQU 18
START   EQU 3000

        ORIG START
        IN INWORD(PTAPE)
        ENTA 0
        LDX INWORD
        NUM
        STA TARGETN

        ENT1 0
        ENTA 2
        STA CAND
        ENTA 1
        STA WHICH

LOOP    CMP1 TARGETN
        JGE DONE

        ENTA 2
        STA DIVISOR
1H      LDA DIVISOR
        MUL DIVISOR
        CMPX CAND
        JG 2F

        ENTA 0
        LDX CAND
        DIV DIVISOR
        JXZ 3F

        LDA DIVISOR
        INCA 1
        STA DIVISOR
        JMP 1B

2H      LDA CAND
        CHAR
        LDA WHICH
        CMPA =1=
        JNE 4F

        STX BUF1
        OUT BUF1(PRINTER)
        ENTA 2
        STA WHICH
        JMP 5F

4H      STX BUF2
        OUT BUF2(PRINTER)
        ENTA 1
        STA WHICH

5H      OUT SPACE(PRINTER)
        INC1 1

3H      LDA CAND
        INCA 1
        STA CAND
        JMP LOOP

DONE    HLT

        ORIG *+2
INWORD  CON 0
TARGETN CON 0
CAND    CON 0
DIVISOR CON 0
WHICH   CON 1
SPACE   ALF "     "
BUF1    CON 0
BUF2    CON 0

        END START`,
		paperTapeInput: '00017'
	},
	{
		id: 'vigenere',
		label: 'Vigenere',
		assembly: `* Vigenere cipher from paper tape to line printer.
* Input format on unit 16:
*   <PASSWORD><spaces-to-next-5-char-block><MESSAGE>
* Password is assumed < 10 chars and has no spaces.
* Message is assumed < 500 chars.
*
* Only A-Z are encrypted; other MIX characters are emitted unchanged.

PTAPE   EQU 16
PRINTER EQU 18
START   EQU 3000

        ORIG START
        ENT2 0
        ENT3 0
        ENT4 0
        ENTA 0
        STA MODE
        STZ OUTWORD

WORDLP  IN INWORD(PTAPE)

        LDA MODE
        CMPA =2=
        JNE PROCWD
        LDA INWORD
        CMPA ZERO
        JE DONE

PROCWD  ENT1 4

CHLP    LDA INWORD
        SRA 0,1
        STZ CUR
        STA CUR(5:5)

        LDA MODE
        CMPA =0=
        JE MODE0
        CMPA =1=
        JE MODE1
        JMP MODE2

MODE0   LDA CUR
        CMPA SPACE
        JE ENDKEY

        CMPA =1=
        JL KEYNON
        CMPA =9=
        JLE KEYR1
        CMPA =11=
        JL KEYNON
        CMPA =19=
        JLE KEYR2
        CMPA =22=
        JL KEYNON
        CMPA =29=
        JLE KEYR3
        JMP KEYNON

KEYR1   SUB =1=
        JMP KEYOK
KEYR2   SUB =2=
        JMP KEYOK
KEYR3   SUB =4=
        JMP KEYOK
KEYNON  ENTA 0
KEYOK   STA KEY0,3
        INC3 1
        JMP NEXTCH

ENDKEY  ENTA 1
        STA MODE
        ST3 KEYLEN
        JMP NEXTCH

MODE1   LDA CUR
        CMPA SPACE
        JE NEXTCH
        ENTA 2
        STA MODE

MODE2   LDA CUR
        CMPA =1=
        JL APPRAW
        CMPA =9=
        JLE ENCR1
        CMPA =11=
        JL APPRAW
        CMPA =19=
        JLE ENCR2
        CMPA =22=
        JL APPRAW
        CMPA =29=
        JLE ENCR3
        JMP APPRAW

ENCR1   SUB =1=
        JMP HAVEI
ENCR2   SUB =2=
        JMP HAVEI
ENCR3   SUB =4=

HAVEI   STA IDX
        LDA KEY0,2
        STA SHIFT
        LDA IDX
        ADD SHIFT

MOD26   CMPA =26=
        JL MAPBK
        SUB =26=
        JMP MOD26

MAPBK   CMPA =9=
        JL BACK1
        CMPA =18=
        JL BACK2
        ADD =4=
        JMP ADVKEY
BACK1   ADD =1=
        JMP ADVKEY
BACK2   ADD =2=

ADVKEY  INC2 1
        CMP2 KEYLEN
        JL APPCHR
        ENT2 0
        JMP APPCHR

APPRAW  LDA CUR

APPCHR  STA ENC
        LDA OUTWORD
        SLA 1
        STA OUTWORD
        LDA ENC
        STA OUTWORD(5:5)
        INC4 1
        CMP4 =5=
        JNE NEXTCH
        OUT OUTWORD(PRINTER)
        STZ OUTWORD
        ENT4 0

NEXTCH  DEC1 1
        J1NN CHLP
        JMP WORDLP

DONE    CMP4 =0=
        JE STOP

PADLP   CMP4 =5=
        JE FLUSH
        LDA OUTWORD
        SLA 1
        STA OUTWORD
        INC4 1
        JMP PADLP

FLUSH   OUT OUTWORD(PRINTER)

STOP    HLT

        ORIG *+2
INWORD  CON 0
OUTWORD CON 0
CUR     CON 0
MODE    CON 0
KEYLEN  CON 0
IDX     CON 0
SHIFT   CON 0
ENC     CON 0
SPACE   CON 0
ZERO    CON 0
KEY0    CON 0
KEY1    CON 0
KEY2    CON 0
KEY3    CON 0
KEY4    CON 0
KEY5    CON 0
KEY6    CON 0
KEY7    CON 0
KEY8    CON 0
KEY9    CON 0

        END START`,
		paperTapeInput: 'LEMON     ATTACK AT DAWN. 123'
	}
];

export const EXAMPLE_PROGRAMS_BY_ID: Record<ExampleProgramId, ExampleProgram> = {
	echo: EXAMPLE_PROGRAMS.find((program) => program.id === 'echo') as ExampleProgram,
	primes: EXAMPLE_PROGRAMS.find((program) => program.id === 'primes') as ExampleProgram,
	vigenere: EXAMPLE_PROGRAMS.find((program) => program.id === 'vigenere') as ExampleProgram
};

export const DEFAULT_EXAMPLE_PROGRAM_ID: ExampleProgramId = 'primes';
