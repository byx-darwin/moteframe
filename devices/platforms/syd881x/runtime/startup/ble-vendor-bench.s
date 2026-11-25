.syntax unified
.cpu cortex-m0
.thumb
/* Same 48-slot Flash application table as the SDK. No VTOR or RAM remap. */
.section .vector_table,"a",%progbits
.global __Vectors
__Vectors:
.word __stack_top
.word Reset
.word NMI_Handler
.word HardFault_Handler
.word 0,0,0,0,0,0,0
.word SVC_Handler
.word 0,0
.word PendSV_Handler
.word SysTick_Handler
.word 0
.word LLC_IRQHandler
.word RTC_IRQHandler
.word SW_IRQHandler
.word I2C0_IRQHandler
.word I2C1_IRQHandler
.word UART0_IRQHandler
.word UART1_IRQHandler
.word TIMER0_IRQHandler
.word TIMER1_IRQHandler
.word TIMER2_IRQHandler
.word TIMER3_IRQHandler
.word GPIO_IRQHandler
.word HID_IRQHandler
.word SPIM_IRQHandler
.word CAP_IRQHandler
.word GPADC_IRQHandler
.word LLC2_IRQHandler
.word ISO_7816_IRQHandler
.word IR_Tx_IRQHandler
.word TOUCH_IRQHandler
.word HPWM_IRQHandler
.word HTIMER_IRQHandler
.word IR_Rx_IRQHandler
.word 0,0,0,0,0,0,0,0
.global __Vectors_End
__Vectors_End:

.section .text.Reset,"ax",%progbits
.global Reset
.type Reset,%function
.thumb_func
Reset:
    cpsid i
    ldr r0, =__stack_top
    mov sp, r0
    ldr r0, =0x20007000
    ldr r1, =__stack_top
    ldr r2, =0xa5a5a5a5
1:
    str r2, [r0]
    adds r0, r0, #4
    cmp r0, r1
    blo 1b
    bl reset_rust
    b .

/* SDK has these default entries too; BLE LLC/SW routing belongs to ROM.
   Never fabricate a vendor handler or install this table over ROM vectors. */
.macro DEFAULT name
    .weak \name
    .thumb_set \name, Default_Handler
.endm
.weak NMI_Handler
.thumb_set NMI_Handler, Fault_Handler
.weak HardFault_Handler
.thumb_set HardFault_Handler, Fault_Handler
DEFAULT SVC_Handler
DEFAULT PendSV_Handler
DEFAULT SysTick_Handler
DEFAULT LLC_IRQHandler
DEFAULT RTC_IRQHandler
DEFAULT SW_IRQHandler
DEFAULT I2C0_IRQHandler
DEFAULT I2C1_IRQHandler
DEFAULT UART0_IRQHandler
DEFAULT UART1_IRQHandler
DEFAULT TIMER0_IRQHandler
DEFAULT TIMER1_IRQHandler
DEFAULT TIMER2_IRQHandler
DEFAULT TIMER3_IRQHandler
DEFAULT HID_IRQHandler
DEFAULT SPIM_IRQHandler
DEFAULT CAP_IRQHandler
DEFAULT GPADC_IRQHandler
DEFAULT LLC2_IRQHandler
DEFAULT ISO_7816_IRQHandler
DEFAULT IR_Tx_IRQHandler
DEFAULT TOUCH_IRQHandler
DEFAULT HPWM_IRQHandler
DEFAULT HTIMER_IRQHandler
DEFAULT IR_Rx_IRQHandler
