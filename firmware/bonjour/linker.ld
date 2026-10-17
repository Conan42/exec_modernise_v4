OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    /* U-Boot's validated kernel address is 0x140000000. The splash lives at
       0x102000000, so the two transfers do not overlap. */
    . = 0x140000000;

    .text : ALIGN(4K)
    {
        KEEP(*(.text._start))
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(4K)
    {
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __stack_bottom = .;
        . += 64K;
        __stack_top = .;
    }

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
    }
}
