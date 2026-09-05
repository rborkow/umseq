#### fetch_exon
def fetch_exon(chrom, st, cigar):
    ''' fetch exon regions defined by cigar. st must be zero based
    return list of tuple of (chrom,st, end)
    '''
    #match = re.compile(r'(\d+)(\D)')
    chrom_st = st
    exon_bound =[]
    for c,s in cigar:   #code and size
        if c==0:        #match
            exon_bound.append((chrom, chrom_st,chrom_st + s))
            chrom_st += s
        elif c==1:      #insertion to ref
            continue
        elif c==2:      #deletion to ref
            chrom_st += s
        elif c==3:      #gap or intron
            chrom_st += s
        elif c==4:      #soft clipping. We do NOT include soft clip as part of exon
            chrom_st += s
        else:
            continue
    return exon_bound
#### annotate_junction
    def annotate_junction(self,refgene,outfile,min_intron=50, q_cut=30):
        '''Annotate splicing junctions in BAM or SAM file. Note that a (long) read might have multiple splicing
        events  (splice multiple times), and the same splicing events can be consolidated into a single
        junction'''
        out_file = outfile + ".junction.xls"
        out_file2 = outfile + ".junction_plot.r"
        if refgene is None:
            print("You must provide reference gene model in bed format.", file=sys.stderr)
            sys.exit(1)
        OUT = open(out_file,'w')
        ROUT = open(out_file2,'w')
        #reading reference gene model
        refIntronStarts=collections.defaultdict(dict)
        refIntronEnds=collections.defaultdict(dict) 
        total_junc = 0
        novel35_junc = 0
        novel3or5_junc = 0
        known_junc = 0
        filtered_junc = 0
        splicing_events=collections.defaultdict(int)    
        print("Reading reference bed file: ",refgene, " ... ", end=' ', file=sys.stderr)
        for line in open(refgene,'r'):
            if line.startswith(('#','track','browser')):continue  
            # Parse fields from gene tabls
            fields = line.split()
            if(len(fields)<12):
                print("Invalid bed line (skipped):",line, end=' ', file=sys.stderr)
                continue
            chrom     = fields[0].upper()
            tx_start = int( fields[1] )
            tx_end   = int( fields[2] )
            if int(fields[9] ==1):
                continue        
            exon_starts = list(map( int, fields[11].rstrip( ',\n' ).split( ',' ) ))
            exon_starts = list(map((lambda x: x + tx_start ), exon_starts))
            exon_ends = list(map( int, fields[10].rstrip( ',\n' ).split( ',' ) ))
            exon_ends = list(map((lambda x, y: x + y ), exon_starts, exon_ends));   
            intron_start = exon_ends[:-1]
            intron_end=exon_starts[1:]
            for i_st,i_end in zip (intron_start, intron_end):
                refIntronStarts[chrom][i_st] =i_st
                refIntronEnds[chrom][i_end] =i_end          
        print("Done", file=sys.stderr)
        #reading input SAM file
        if self.bam_format:print("Load BAM file ... ", end=' ', file=sys.stderr)
        else:print("Load SAM file ... ", end=' ', file=sys.stderr)
        try:
            while(1):
                aligned_read = next(self.samfile)
                if aligned_read.is_qcfail:continue          #skip low quanlity                  
                if aligned_read.is_duplicate:continue       #skip duplicate read
                if aligned_read.is_secondary:continue       #skip non primary hit
                if aligned_read.is_unmapped:continue        #skip unmap read
                if aligned_read.mapq < q_cut:continue
                chrom = self.samfile.getrname(aligned_read.tid).upper()
                hit_st = aligned_read.pos
                intron_blocks = bam_cigar.fetch_intron(chrom, hit_st, aligned_read.cigar)           
                if len(intron_blocks)==0:
                    continue
                for intrn in intron_blocks:
                    total_junc +=1
                    if intrn[2] - intrn[1] < min_intron:
                        filtered_junc += 1
                        continue
                    splicing_events[intrn[0] + ":" + str(intrn[1]) + ":" + str(intrn[2])] += 1
                    if (intrn[1] in refIntronStarts[chrom] and intrn[2] in refIntronEnds[chrom]):
                        known_junc +=1                                                                      #known both
                    elif (intrn[1] not in refIntronStarts[chrom] and intrn[2] not in refIntronEnds[chrom]):
                        novel35_junc +=1                                                                
                    else:
                        novel3or5_junc +=1
        except StopIteration:
            print("Done", file=sys.stderr)
        print("total = " + str(total_junc))
        if total_junc == 0:
            print("No splice junction found.", file=sys.stderr)
            sys.exit()
        #self.f.seek(0)
        print('pdf(\"%s\")' % (outfile + ".splice_events.pdf"), file=ROUT)
        print("events=c(" + ','.join([str(i*100.0/total_junc) for i in (novel3or5_junc,novel35_junc,known_junc)])+ ')', file=ROUT)
        print('pie(events,col=c(2,3,4),init.angle=30,angle=c(60,120,150),density=c(70,70,70),main="splicing events",labels=c("partial_novel %d%%","complete_novel %d%%","known %d%%"))' % (round(novel3or5_junc*100.0/total_junc),round(novel35_junc*100.0/total_junc),round(known_junc*100.0/total_junc)), file=ROUT)
        print("dev.off()", file=ROUT)
        print("\n===================================================================", file=sys.stderr)
        print("Total splicing  Events:\t" + str(total_junc), file=sys.stderr)
        print("Known Splicing Events:\t" + str(known_junc), file=sys.stderr)
        print("Partial Novel Splicing Events:\t" + str(novel3or5_junc), file=sys.stderr)
        print("Novel Splicing Events:\t" + str(novel35_junc), file=sys.stderr)
        print("Filtered Splicing Events:\t" + str(filtered_junc), file=sys.stderr)        
        #reset variables
        total_junc =0
        novel35_junc =0
        novel3or5_junc =0
        known_junc =0
        print("chrom\tintron_st(0-based)\tintron_end(1-based)\tread_count\tannotation", file=OUT)
        for i in splicing_events:
            total_junc += 1
            (chrom, i_st, i_end) = i.split(":")
            print('\t'.join([chrom.replace("CHR","chr"),i_st,i_end]) + '\t' + str(splicing_events[i]) + '\t', end=' ', file=OUT)
            i_st = int(i_st)
            i_end = int(i_end)
            if (i_st in refIntronStarts[chrom] and i_end in refIntronEnds[chrom]):
                print("annotated", file=OUT)
                known_junc +=1
            elif (i_st not in refIntronStarts[chrom] and i_end not in refIntronEnds[chrom]):
                print('complete_novel', file=OUT)
                novel35_junc +=1
            else:
                print('partial_novel', file=OUT)
                novel3or5_junc +=1
        if total_junc ==0:
            print("No splice read found", file=sys.stderr)
            sys.exit(1)
        print("\nTotal splicing  Junctions:\t" + str(total_junc), file=sys.stderr)
        print("Known Splicing Junctions:\t" + str(known_junc), file=sys.stderr)
        print("Partial Novel Splicing Junctions:\t" + str(novel3or5_junc), file=sys.stderr)
        print("Novel Splicing Junctions:\t" + str(novel35_junc), file=sys.stderr)
        print("\n===================================================================", file=sys.stderr)
        print('pdf(\"%s\")' % (outfile + ".splice_junction.pdf"), file=ROUT)
        print("junction=c(" + ','.join([str(i*100.0/total_junc) for i in (novel3or5_junc,novel35_junc,known_junc,)])+ ')', file=ROUT)
        print('pie(junction,col=c(2,3,4),init.angle=30,angle=c(60,120,150),density=c(70,70,70),main="splicing junctions",labels=c("partial_novel %d%%","complete_novel %d%%","known %d%%"))' % (round(novel3or5_junc*100.0/total_junc),round(novel35_junc*100.0/total_junc),round(known_junc*100.0/total_junc)), file=ROUT)
        print("dev.off()", file=ROUT)
        #print >>ROUT, "mat=matrix(c(events,junction),byrow=T,ncol=3)"
        #print >>ROUT, 'barplot(mat,beside=T,ylim=c(0,100),names=c("known","partial\nnovel","complete\nnovel"),legend.text=c("splicing events","splicing junction"),ylab="Percent")'
    def saturation_junction(self,refgene,outfile=None,sample_start=5,sample_step=5,sample_end=100,min_intron=50,recur=1, q_cut=30):
        '''check if an RNA-seq experiment is saturated in terms of detecting known splicing junction'''
        out_file = outfile + ".junctionSaturation_plot.r"
        if refgene is None:
            print("You must provide reference gene model in bed format.", file=sys.stderr)
            sys.exit(1)
        OUT = open(out_file,'w')
        knownSpliceSites= set()
        chrom_list=set()
        print("reading reference bed file: ",refgene, " ... ", end=' ', file=sys.stderr)
        for line in open(refgene,'r'):
            if line.startswith(('#','track','browser')):continue  
            fields = line.split()
            if(len(fields)<12):
                print("Invalid bed line (skipped):",line, end=' ', file=sys.stderr)
                continue
            chrom     = fields[0].upper()
            chrom_list.add(chrom)
            tx_start = int( fields[1] )
            tx_end   = int( fields[2] )
            if int(fields[9] ==1):
                continue        
            exon_starts = list(map( int, fields[11].rstrip( ',\n' ).split( ',' ) ))
            exon_starts = list(map((lambda x: x + tx_start ), exon_starts))
            exon_ends = list(map( int, fields[10].rstrip( ',\n' ).split( ',' ) ))
            exon_ends = list(map((lambda x, y: x + y ), exon_starts, exon_ends));   
            intron_start = exon_ends[:-1]
            intron_end=exon_starts[1:]
            for st,end in zip (intron_start, intron_end):
                knownSpliceSites.add(chrom + ":" + str(st) + "-" + str(end))
        print("Done! Total "+str(len(knownSpliceSites)) + " known splicing junctions.", file=sys.stderr)
        samSpliceSites=[]
        intron_start=[]
        intron_end=[]
        uniqSpliceSites=collections.defaultdict(int)
        if self.bam_format:print("Load BAM file ... ", end=' ', file=sys.stderr)
        else:print("Load SAM file ... ", end=' ', file=sys.stderr)
        try:
            while(1):
                aligned_read = next(self.samfile)
                try:
                    chrom = self.samfile.getrname(aligned_read.tid).upper()
                except:
                    continue
                if chrom not in chrom_list:
                    continue                
                if aligned_read.is_qcfail:continue          #skip low quanlity                  
                if aligned_read.is_duplicate:continue       #skip duplicate read
                if aligned_read.is_secondary:continue       #skip non primary hit
                if aligned_read.is_unmapped:continue        #skip unmap read
                if aligned_read.mapq < q_cut: continue
                hit_st = aligned_read.pos
                intron_blocks = bam_cigar.fetch_intron(chrom, hit_st, aligned_read.cigar)           
                if len(intron_blocks)==0:
                    continue
                for intrn in intron_blocks:
                    if intrn[2] - intrn[1] < min_intron:continue
                    samSpliceSites.append(intrn[0] + ":" + str(intrn[1]) + "-" + str(intrn[2]))
        except StopIteration:
            print("Done", file=sys.stderr)
        print("shuffling alignments ...", end=' ', file=sys.stderr)
        random.shuffle(samSpliceSites)
        print("Done", file=sys.stderr)
        SR_num = len(samSpliceSites)
        sample_size=0
        all_junctionNum = 0 
        known_junc=[]
        all_junc=[]
        unknown_junc=[]
        tmp=list(range(sample_start,sample_end,sample_step))
        tmp.append(100)
        for pertl in tmp:   #[5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95,100]
            knownSpliceSites_num = 0
            index_st = int(SR_num * ((pertl - sample_step)/100.0))
            index_end = int(SR_num * (pertl/100.0))
            if index_st < 0: index_st = 0
            sample_size += index_end -index_st
            print("sampling " + str(pertl) +"% (" + str(sample_size) + ") splicing reads.", end=' ', file=sys.stderr)
            for i in range(index_st, index_end):
                uniqSpliceSites[samSpliceSites[i]] +=1  
            all_junctionNum = len(list(uniqSpliceSites.keys()))
            all_junc.append(str(all_junctionNum))
            print(str(all_junctionNum) + " splicing junctions.", end=' ', file=sys.stderr)
            known_junctionNum = 0
            for sj in uniqSpliceSites:
                if sj in knownSpliceSites and uniqSpliceSites[sj] >= recur:
                    known_junctionNum +=1
            print(str(known_junctionNum) + " known splicing junctions.", end=' ', file=sys.stderr)
            known_junc.append(str(known_junctionNum))
            unknown_junctionNum = 0
            for sj in uniqSpliceSites:
                if sj not in knownSpliceSites:
                    unknown_junctionNum +=1
            unknown_junc.append(str(unknown_junctionNum))
            print(str(unknown_junctionNum) + " novel splicing junctions.", file=sys.stderr)
        print("pdf(\'%s\')" % (outfile + '.junctionSaturation_plot.pdf'), file=OUT)
        print("x=c(" + ','.join([str(i) for i in tmp]) + ')', file=OUT)
        print("y=c(" + ','.join(known_junc) + ')', file=OUT)
        print("z=c(" + ','.join(all_junc) + ')', file=OUT)
        print("w=c(" + ','.join(unknown_junc) + ')', file=OUT)
        print("m=max(%d,%d,%d)" % (int(int(known_junc[-1])/1000), int(int(all_junc[-1])/1000),int(int(unknown_junc[-1])/1000)), file=OUT)
        print("n=min(%d,%d,%d)" % (int(int(known_junc[0])/1000), int(int(all_junc[0])/1000),int(int(unknown_junc[0])/1000)), file=OUT)
        print("plot(x,z/1000,xlab='percent of total reads',ylab='Number of splicing junctions (x1000)',type='o',col='blue',ylim=c(n,m))", file=OUT)
        print("points(x,y/1000,type='o',col='red')", file=OUT)
        print("points(x,w/1000,type='o',col='green')", file=OUT)
        print('legend(5,%d, legend=c("All junctions","known junctions", "novel junctions"),col=c("blue","red","green"),lwd=1,pch=1)' % int(int(all_junc[-1])/1000), file=OUT)
        print("dev.off()", file=OUT)
    def configure_experiment(self,refbed,sample_size, q_cut = 30):
        '''Given a BAM/SAM file, this function will try to guess the RNA-seq experiment:
            1) single-end or pair-end
            2) strand_specific or not
            3) if it is strand-specific, what's the strand_ness of the protocol
        '''
        count =0
        p_strandness=collections.defaultdict(int)
        s_strandness=collections.defaultdict(int)
        gene_ranges={}
        print("Reading reference gene model " + refbed + ' ...', end=' ', file=sys.stderr)
        for line in open(refbed,'r'):
            try:
                if line.startswith(('#','track','browser')):continue  
                fields = line.split()
                chrom     = fields[0]
                txStart  = int( fields[1] )
                txEnd    = int( fields[2] )
                geneName      = fields[3]
                strand    = fields[5]
            except:
                print("[NOTE:input bed must be 12-column] skipped this line: " + line, file=sys.stderr)
                continue
            if chrom not in gene_ranges:
                gene_ranges[chrom]=Intersecter()
            gene_ranges[chrom].insert(txStart,txEnd,strand)                         
        print("Done", file=sys.stderr)      
        print("Loading SAM/BAM file ... ", end=' ', file=sys.stderr)
        try:
            while(1):
                if count >= sample_size:
                    break
                aligned_read = next(self.samfile)
                if aligned_read.is_qcfail:          #skip low quanlity
                    continue
                if aligned_read.is_duplicate:       #skip duplicate read
                    continue
                if aligned_read.is_secondary:       #skip non primary hit
                    continue
                if aligned_read.is_unmapped:        #skip unmap read
                    continue        
                if aligned_read.mapq < q_cut:
                    continue                                                        
                chrom = self.samfile.getrname(aligned_read.tid)
                if aligned_read.is_paired:
                    if aligned_read.is_read1:
                        read_id = '1'
                    if aligned_read.is_read2:
                        read_id = '2'
                    if aligned_read.is_reverse:
                        map_strand = '-'
                    else:
                        map_strand = '+'
                    readStart = aligned_read.pos
                    readEnd = readStart + aligned_read.qlen
                    if chrom in gene_ranges:
                        tmp = set(gene_ranges[chrom].find(readStart,readEnd))
                        if len(tmp) == 0: continue
                        strand_from_gene = ':'.join(tmp)
                        p_strandness[read_id + map_strand + strand_from_gene]+=1    
                        count += 1
                else:
                    if aligned_read.is_reverse:
                        map_strand = '-'
                    else:
                        map_strand = '+'                    
                    readStart = aligned_read.pos
                    readEnd = readStart + aligned_read.qlen
                    if chrom in gene_ranges:
                        tmp = set(gene_ranges[chrom].find(readStart,readEnd))
                        if len(tmp) == 0: continue
                        strand_from_gene = ':'.join(tmp)
                        s_strandness[map_strand + strand_from_gene]+=1
                        count += 1
        except StopIteration:
            print("Finished", file=sys.stderr)      
        print("Total " + str(count) + " usable reads were sampled", file=sys.stderr)
        protocol="unknown"
        strandness=None
        spec1=0.0
        spec2=0.0
        other=0.0
        if len(p_strandness) >0 and len(s_strandness) ==0 :
            protocol="PairEnd"
            spec1= (p_strandness['1++'] + p_strandness['1--'] + p_strandness['2+-'] + p_strandness['2-+'])/float(sum(p_strandness.values()))
            spec2= (p_strandness['1+-'] + p_strandness['1-+'] + p_strandness['2++'] + p_strandness['2--'])/float(sum(p_strandness.values()))
            other = 1-spec1-spec2
        elif len(s_strandness) >0 and len(p_strandness) ==0 :
            protocol="SingleEnd"
            spec1 = (s_strandness['++'] + s_strandness['--'])/float(sum(s_strandness.values()))
            spec2 = (s_strandness['+-'] + s_strandness['-+'])/float(sum(s_strandness.values()))
            other = 1-spec1-spec2
        else:
            protocol="Mixture"
            spec1 = 0
            spec2 = 0
            other = 0
        return [protocol,spec1,spec2,other]
    def mRNA_inner_distance(self,outfile,refbed,low_bound=0,up_bound=1000,step=10,sample_size=1000000, q_cut=30):
        '''estimate the inner distance of mRNA pair end fragment. fragment size = insert_size + 2 x read_length'''
        out_file1 = outfile + ".inner_distance.txt" 
        out_file2 = outfile + ".inner_distance_freq.txt"
        out_file3 = outfile + ".inner_distance_plot.r"
        FO=open(out_file1,'w')
        FQ=open(out_file2,'w')
        RS=open(out_file3,'w')
        fchrom="chr100"     #this is the fake chromosome
        ranges={}
        ranges[fchrom]=Intersecter()
        window_left_bound = list(range(low_bound,up_bound,step))
        frag_size=0
        inner_distance_bitsets=BinnedBitSet()
        tmp = BinnedBitSet()
        tmp.set_range(0,0)
        pair_num=0
        sizes=[]
        counts=[]
        count=0
        print("Get exon regions from " + refbed + " ...", file=sys.stderr)
        bed_obj = BED.ParseBED(refbed)
        ref_exons = []
        for exn in bed_obj.getExon():
            ref_exons.append([exn[0].upper(), exn[1], exn[2]])
        exon_bitsets = binned_bitsets_from_list(ref_exons)
        transcript_ranges = {}
        for i_chr, i_st, i_end, i_strand, i_name in bed_obj.getTranscriptRanges():
            i_chr = i_chr.upper()
            if i_chr not in transcript_ranges:
                transcript_ranges[i_chr] = Intersecter()
            else:
                transcript_ranges[i_chr].add_interval(Interval(i_st, i_end, value=i_name))
        if self.bam_format:print("Load BAM file ... ", end=' ', file=sys.stderr)
        else:print("Load SAM file ... ", end=' ', file=sys.stderr)
        try:
            while(1):
                if pair_num >= sample_size:
                    break
                splice_intron_size=0
                aligned_read = next(self.samfile)
                if aligned_read.is_qcfail:continue          #skip low quanlity                  
                if aligned_read.is_duplicate:continue       #skip duplicate read
                if aligned_read.is_secondary:continue       #skip non primary hit
                if aligned_read.is_unmapped:continue        #skip unmap read
                if not aligned_read.is_paired: continue     #skip single map read
                if aligned_read.mate_is_unmapped:continue   #
                if aligned_read.mapq < q_cut:continue
                read1_len = aligned_read.qlen
                read1_start = aligned_read.pos
                read2_start = aligned_read.mpos     #0-based, not included
                if read2_start < read1_start:
                    continue                                #because BAM file is sorted, mate_read is already processed if its coordinate is smaller
                if  read2_start == read1_start and aligned_read.is_read1:
                    inner_distance = 0
                    continue
                pair_num +=1
                R_read1_ref = self.samfile.getrname(aligned_read.tid)
                R_read2_ref = self.samfile.getrname(aligned_read.rnext)
                if R_read1_ref != R_read2_ref:
                    FO.write(aligned_read.qname + '\t' + 'NA' + '\tsameChrom=No\n') #reads mapped to different chromosomes
                    continue
                chrom = self.samfile.getrname(aligned_read.tid).upper()
                intron_blocks = bam_cigar.fetch_intron(chrom, read1_start, aligned_read.cigar)              
                for intron in intron_blocks:
                    splice_intron_size += intron[2] - intron[1]
                read1_end = read1_start + read1_len + splice_intron_size        
                if read2_start >= read1_end:
                    inner_distance = read2_start - read1_end
                else:
                    exon_positions = []
                    exon_blocks = bam_cigar.fetch_exon(chrom, read1_start,aligned_read.cigar)
                    for ex in exon_blocks:
                        for i in range(ex[1]+1,ex[2]+1):
                            exon_positions.append(i)
                    inner_distance = -len([i for i in exon_positions if i > read2_start and i <= read1_end])
                read1_gene_names = set()    #read1_end
                try:
                    for gene in transcript_ranges[chrom].find(read1_end-1, read1_end):  #gene: Interval(0, 10, value=a)
                        read1_gene_names.add(gene.value)
                except:
                    pass
                read2_gene_names = set()    #read2_start
                try:
                    for gene in transcript_ranges[chrom].find(read2_start, read2_start +1): #gene: Interval(0, 10, value=a)
                        read2_gene_names.add(gene.value)
                except:
                    pass
                if len(read1_gene_names.intersection(read2_gene_names)) == 0:   # no common gene
                    FO.write(aligned_read.qname + '\t' + str(inner_distance) + '\tsameTranscript=No,dist=genomic\n')        #reads mapped to different gene
                    ranges[fchrom].add_interval( Interval( inner_distance-1, inner_distance ) )     
                    continue            
                if inner_distance > 0: 
                    if chrom in exon_bitsets:
                        size =0 
                        inner_distance_bitsets.set_range(read1_end, read2_start-read1_end)
                        inner_distance_bitsets.iand(exon_bitsets[chrom])
                        end=0
                        while 1:
                            start = inner_distance_bitsets.next_set( end )
                            if start == inner_distance_bitsets.size: break
                            end = inner_distance_bitsets.next_clear( start )
                            size += (end - start)
                        inner_distance_bitsets.iand(tmp)                                            #clear BinnedBitSet
                        if size == inner_distance:
                            FO.write(aligned_read.qname + '\t' + str(size) + '\tsameTranscript=Yes,sameExon=Yes,dist=mRNA\n')
                            ranges[fchrom].add_interval( Interval( size-1, size ) )
                        elif size > 0 and size < inner_distance:
                            FO.write(aligned_read.qname + '\t' + str(size) + '\tsameTranscript=Yes,sameExon=No,dist=mRNA\n')
                            ranges[fchrom].add_interval( Interval( size-1, size ) ) 
                        elif size <= 0:
                            FO.write(aligned_read.qname + '\t' + str(inner_distance) + '\tsameTranscript=Yes,nonExonic=Yes,dist=genomic\n')
                            ranges[fchrom].add_interval( Interval( inner_distance-1, inner_distance ) )     
                    else:
                        FO.write(aligned_read.qname + '\t' + str(inner_distance) + '\tunknownChromosome,dist=genomic')
                        ranges[fchrom].add_interval( Interval( inner_distance-1, inner_distance ) )
                else:
                    FO.write(aligned_read.qname + '\t' + str(inner_distance) + '\treadPairOverlap\n')
                    ranges[fchrom].add_interval( Interval( inner_distance-1, inner_distance ) )
        except StopIteration:
            print("Done", file=sys.stderr)
        print("Total read pairs  used " + str(pair_num), file=sys.stderr)
        if pair_num==0:
            print("Cannot find paired reads", file=sys.stderr)
            sys.exit(0)
        for st in window_left_bound:
            sizes.append(str(st + step/2))
            count = str(len(ranges[fchrom].find(st,st + step)))
            counts.append(count)
            print(str(st) + '\t' + str(st+step) +'\t' + count, file=FQ)     
        print("out_file = \'%s\'" % outfile, file=RS)
        print("pdf(\'%s\')" % (outfile + ".inner_distance_plot.pdf"), file=RS)
        print('fragsize=rep(c(' + ','.join(sizes) + '),' + 'times=c(' + ','.join(counts) + '))', file=RS)
        print('frag_sd = sd(fragsize)', file=RS)
        print('frag_mean = mean(fragsize)', file=RS)
        print('frag_median = median(fragsize)', file=RS)
        print('write(x=c("Name","Mean","Median","sd"), sep="\t", file=stdout(),ncolumns=4)', file=RS)
        print('write(c(out_file,frag_mean,frag_median,frag_sd),sep="\t", file=stdout(),ncolumns=4)', file=RS)
        print('hist(fragsize,probability=T,breaks=%d,xlab="mRNA insert size (bp)",main=paste(c("Mean=",frag_mean,";","SD=",frag_sd),collapse=""),border="blue")' % len(window_left_bound), file=RS)
        print("lines(density(fragsize,bw=%d),col='red')" % (2*step), file=RS)
        print("dev.off()", file=RS)
        FO.close()
        FQ.close()
        RS.close()
Traceback (most recent call last):
  File "<string>", line 5, in <module>
ModuleNotFoundError: No module named 'RSeQC'
'''
Check reads distribution over exon, intron, UTR, intergenic ... etc
The following reads will be skipped:
	qc_failed
	PCR duplicate
	Unmapped
	Non-primary (or secondary)	
'''
import os,sys
if sys.version_info[0] != 3:
	print("\nYou are using python" + str(sys.version_info[0]) + '.' + str(sys.version_info[1]) + " This verion of RSeQC needs python3!\n", file=sys.stderr)
	sys.exit()	
import re
import string
from optparse import OptionParser
import warnings
import collections
import math
from bx.bitset import *
from bx.bitset_builders import *
from bx.intervals import *
from bx.binned_array import BinnedArray
from bx_extras.fpconst import isNaN
from bx.bitset_utils import *
from qcmodule import BED
from qcmodule import SAM
from qcmodule import bam_cigar
__author__ = "Liguo Wang"
__copyright__ = "Copyleft"
__credits__ = []
__license__ = "GPL"
__version__="5.0.4"
__maintainer__ = "Liguo Wang"
__email__ = "wang.liguo@mayo.edu"
__status__ = "Production"
def cal_size(list):
	'''calcualte bed list total size'''
	size=0
	for l in list:
		size += l[2] - l[1]
	return size
def foundone(chrom,ranges, st, end):
	found = 0
	if chrom in ranges:
		found = len(ranges[chrom].find(st,end))
	return found
def build_bitsets(list):
	'''build intevalTree from list'''
	ranges={}
	for l in list:
		chrom =l[0].upper()
		st = int(l[1])
		end = int(l[2])
		if chrom not in ranges:
			ranges[chrom] = Intersecter()
		ranges[chrom].add_interval( Interval( st, end ) )
	return ranges
def process_gene_model(gene_model):
	print("processing " + gene_model + ' ...', end=' ', file=sys.stderr)
	obj = BED.ParseBED(gene_model)
	utr_3 = obj.getUTR(utr=3)
	utr_5 = obj.getUTR(utr=5)
	cds_exon = obj.getCDSExon()
	intron = obj.getIntron()
	intron = BED.unionBed3(intron)
	cds_exon=BED.unionBed3(cds_exon)
	utr_5 = BED.unionBed3(utr_5)
	utr_3 = BED.unionBed3(utr_3)
	utr_5 = BED.subtractBed3(utr_5,cds_exon)
	utr_3 = BED.subtractBed3(utr_3,cds_exon)
	intron = BED.subtractBed3(intron,cds_exon)
	intron = BED.subtractBed3(intron,utr_5)
	intron = BED.subtractBed3(intron,utr_3)
	intergenic_up_1kb = obj.getIntergenic(direction="up",size=1000)
	intergenic_down_1kb = obj.getIntergenic(direction="down",size=1000)
	intergenic_up_5kb = obj.getIntergenic(direction="up",size=5000)
	intergenic_down_5kb = obj.getIntergenic(direction="down",size=5000)	
	intergenic_up_10kb = obj.getIntergenic(direction="up",size=10000)
	intergenic_down_10kb = obj.getIntergenic(direction="down",size=10000)
	intergenic_up_1kb=BED.unionBed3(intergenic_up_1kb)
	intergenic_up_5kb=BED.unionBed3(intergenic_up_5kb)
	intergenic_up_10kb=BED.unionBed3(intergenic_up_10kb)
	intergenic_down_1kb=BED.unionBed3(intergenic_down_1kb)
	intergenic_down_5kb=BED.unionBed3(intergenic_down_5kb)
	intergenic_down_10kb=BED.unionBed3(intergenic_down_10kb)	
	intergenic_up_1kb=BED.subtractBed3(intergenic_up_1kb,cds_exon)
	intergenic_up_1kb=BED.subtractBed3(intergenic_up_1kb,utr_5)
	intergenic_up_1kb=BED.subtractBed3(intergenic_up_1kb,utr_3)
	intergenic_up_1kb=BED.subtractBed3(intergenic_up_1kb,intron)
	intergenic_down_1kb=BED.subtractBed3(intergenic_down_1kb,cds_exon)
	intergenic_down_1kb=BED.subtractBed3(intergenic_down_1kb,utr_5)
	intergenic_down_1kb=BED.subtractBed3(intergenic_down_1kb,utr_3)
	intergenic_down_1kb=BED.subtractBed3(intergenic_down_1kb,intron)	
	intergenic_up_5kb=BED.subtractBed3(intergenic_up_5kb,cds_exon)
	intergenic_up_5kb=BED.subtractBed3(intergenic_up_5kb,utr_5)
	intergenic_up_5kb=BED.subtractBed3(intergenic_up_5kb,utr_3)
	intergenic_up_5kb=BED.subtractBed3(intergenic_up_5kb,intron)
	intergenic_down_5kb=BED.subtractBed3(intergenic_down_5kb,cds_exon)
	intergenic_down_5kb=BED.subtractBed3(intergenic_down_5kb,utr_5)
	intergenic_down_5kb=BED.subtractBed3(intergenic_down_5kb,utr_3)
	intergenic_down_5kb=BED.subtractBed3(intergenic_down_5kb,intron)	
	intergenic_up_10kb=BED.subtractBed3(intergenic_up_10kb,cds_exon)
	intergenic_up_10kb=BED.subtractBed3(intergenic_up_10kb,utr_5)
	intergenic_up_10kb=BED.subtractBed3(intergenic_up_10kb,utr_3)
	intergenic_up_10kb=BED.subtractBed3(intergenic_up_10kb,intron)
	intergenic_down_10kb=BED.subtractBed3(intergenic_down_10kb,cds_exon)
	intergenic_down_10kb=BED.subtractBed3(intergenic_down_10kb,utr_5)
	intergenic_down_10kb=BED.subtractBed3(intergenic_down_10kb,utr_3)
	intergenic_down_10kb=BED.subtractBed3(intergenic_down_10kb,intron)	
	cds_exon_ranges = build_bitsets(cds_exon)
	utr_5_ranges = build_bitsets(utr_5)
	utr_3_ranges = build_bitsets(utr_3)
	intron_ranges = build_bitsets(intron)
	interg_ranges_up_1kb_ranges = build_bitsets(intergenic_up_1kb)
	interg_ranges_up_5kb_ranges = build_bitsets(intergenic_up_5kb)
	interg_ranges_up_10kb_ranges = build_bitsets(intergenic_up_10kb)
	interg_ranges_down_1kb_ranges = build_bitsets(intergenic_down_1kb)
	interg_ranges_down_5kb_ranges = build_bitsets(intergenic_down_5kb)
	interg_ranges_down_10kb_ranges = build_bitsets(intergenic_down_10kb)
	exon_size = cal_size(cds_exon)
	intron_size = cal_size(intron)
	utr3_size = cal_size(utr_3)
	utr5_size = cal_size(utr_5)
	int_up1k_size = cal_size(intergenic_up_1kb)
	int_up5k_size = cal_size(intergenic_up_5kb)
	int_up10k_size = cal_size(intergenic_up_10kb)
	int_down1k_size = cal_size(intergenic_down_1kb)
	int_down5k_size = cal_size(intergenic_down_5kb)
	int_down10k_size = cal_size(intergenic_down_10kb)
	print("Done", file=sys.stderr)
	return (cds_exon_ranges,intron_ranges,utr_5_ranges,utr_3_ranges,\
			interg_ranges_up_1kb_ranges,interg_ranges_up_5kb_ranges,interg_ranges_up_10kb_ranges,\
			interg_ranges_down_1kb_ranges,interg_ranges_down_5kb_ranges,interg_ranges_down_10kb_ranges,\
			exon_size,intron_size,utr5_size,utr3_size,\
			int_up1k_size,int_up5k_size,int_up10k_size,\
			int_down1k_size,int_down5k_size,int_down10k_size)
def main():
	usage="%prog [options]" + '\n' + __doc__ + "\n"
	parser = OptionParser(usage,version="%prog " + __version__)
	parser.add_option("-i","--input-file",action="store",type="string",dest="input_file",help="Alignment file in BAM or SAM format.")
	parser.add_option("-r","--refgene",action="store",type="string",dest="ref_gene_model",help="Reference gene model in bed format.")
	(options,args)=parser.parse_args()
	if not (options.input_file and options.ref_gene_model):
		parser.print_help()
		sys.exit(0)
	if not os.path.exists(options.ref_gene_model):
		print('\n\n' + options.ref_gene_model + " does NOT exists" + '\n', file=sys.stderr)
		sys.exit(0)
	if not os.path.exists(options.input_file):
		print('\n\n' + options.input_file + " does NOT exists" + '\n', file=sys.stderr)
		sys.exit(0)		
	(cds_exon_r, intron_r, utr_5_r, utr_3_r,\
	intergenic_up_1kb_r,intergenic_up_5kb_r,intergenic_up_10kb_r,\
	intergenic_down_1kb_r,intergenic_down_5kb_r,intergenic_down_10kb_r,\
	cds_exon_base,intron_base,utr_5_base,utr_3_base,\
	intergenic_up1kb_base,intergenic_up5kb_base,intergenic_up10kb_base,\
	intergenic_down1kb_base,intergenic_down5kb_base,intergenic_down10kb_base) = process_gene_model(options.ref_gene_model)
	intron_read=0
	cds_exon_read=0
	utr_5_read=0
	utr_3_read=0
	intergenic_up1kb_read=0
	intergenic_down1kb_read=0
	intergenic_up5kb_read=0
	intergenic_down5kb_read=0
	intergenic_up10kb_read=0
	intergenic_down10kb_read=0
	totalReads=0
	totalFrags=0
	unAssignFrags=0
	obj = SAM.ParseBAM(options.input_file)
	R_qc_fail=0
	R_duplicate=0
	R_nonprimary=0
	R_unmap=0
	print("processing " + options.input_file + " ...", end=' ', file=sys.stderr)
	try:
		while(1):
			aligned_read = next(obj.samfile)
			if aligned_read.is_qcfail:			#skip QC fail read
				R_qc_fail +=1
				continue
			if aligned_read.is_duplicate:		#skip duplicate read
				R_duplicate +=1
				continue
			if aligned_read.is_secondary:		#skip non primary hit
				R_nonprimary +=1
				continue
			if aligned_read.is_unmapped:		#skip unmap read
				R_unmap +=1
				continue		
			totalReads +=1
			chrom = obj.samfile.getrname(aligned_read.tid)
			chrom=chrom.upper()
			exons = bam_cigar.fetch_exon(chrom, aligned_read.pos, aligned_read.cigar)
			totalFrags += len(exons)
			for exn in exons:
				mid = int(exn[1]) + int((int(exn[2]) - int(exn[1]))/2)
				if foundone(chrom,cds_exon_r,mid,mid) > 0:
					cds_exon_read += 1
					continue
				elif foundone(chrom,utr_5_r,mid,mid) >0 and foundone(chrom,utr_3_r,mid,mid) == 0:
					utr_5_read += 1
					continue
				elif foundone(chrom,utr_3_r,mid,mid) >0 and foundone(chrom,utr_5_r,mid,mid) == 0:
					utr_3_read += 1
					continue
				elif foundone(chrom,utr_3_r,mid,mid) >0 and foundone(chrom,utr_5_r,mid,mid) > 0:
					unAssignFrags +=1
					continue
				elif foundone(chrom,intron_r,mid,mid) > 0:
					intron_read += 1
					continue
				elif foundone(chrom,intergenic_up_10kb_r,mid,mid) >0 and foundone(chrom,intergenic_down_10kb_r,mid,mid) > 0:
					unAssignFrags +=1
					continue					
				elif foundone(chrom,intergenic_up_1kb_r,mid,mid) >0:
					intergenic_up1kb_read += 1
					intergenic_up5kb_read += 1
					intergenic_up10kb_read += 1
				elif foundone(chrom,intergenic_up_5kb_r,mid,mid) >0:
					intergenic_up5kb_read += 1
					intergenic_up10kb_read += 1
				elif foundone(chrom,intergenic_up_10kb_r,mid,mid) >0:
					intergenic_up10kb_read += 1
				elif foundone(chrom,intergenic_down_1kb_r,mid,mid) >0:
					intergenic_down1kb_read += 1
					intergenic_down5kb_read += 1
					intergenic_down10kb_read += 1
				elif foundone(chrom,intergenic_down_5kb_r,mid,mid) >0:
					intergenic_down5kb_read += 1
					intergenic_down10kb_read += 1
				elif foundone(chrom,intergenic_down_10kb_r,mid,mid) >0:
					intergenic_down10kb_read += 1	
				else:
					unAssignFrags +=1
	except StopIteration:
		print("Finished\n", file=sys.stderr)				
	print("%-30s%d" % ("Total Reads",totalReads))
	print("%-30s%d" % ("Total Tags",totalFrags))
	print("%-30s%d" % ("Total Assigned Tags",totalFrags-unAssignFrags))
	print("=====================================================================")
	print("%-20s%-20s%-20s%-20s" % ('Group','Total_bases','Tag_count','Tags/Kb'))
	print("%-20s%-20d%-20d%-18.2f" % ('CDS_Exons',cds_exon_base,cds_exon_read,cds_exon_read*1000.0/(cds_exon_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("5'UTR_Exons",utr_5_base,utr_5_read, utr_5_read*1000.0/(utr_5_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("3'UTR_Exons",utr_3_base,utr_3_read, utr_3_read*1000.0/(utr_3_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("Introns",intron_base,intron_read,intron_read*1000.0/(intron_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("TSS_up_1kb",intergenic_up1kb_base, intergenic_up1kb_read, intergenic_up1kb_read*1000.0/(intergenic_up1kb_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("TSS_up_5kb",intergenic_up5kb_base, intergenic_up5kb_read, intergenic_up5kb_read*1000.0/(intergenic_up5kb_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("TSS_up_10kb",intergenic_up10kb_base, intergenic_up10kb_read, intergenic_up10kb_read*1000.0/(intergenic_up10kb_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("TES_down_1kb",intergenic_down1kb_base, intergenic_down1kb_read, intergenic_down1kb_read*1000.0/(intergenic_down1kb_base+1)))
	print("%-20s%-20d%-20d%-18.2f" % ("TES_down_5kb",intergenic_down5kb_base, intergenic_down5kb_read, intergenic_down5kb_read*1000.0/(intergenic_down5kb_base+1)))	
	print("%-20s%-20d%-20d%-18.2f" % ("TES_down_10kb",intergenic_down10kb_base, intergenic_down10kb_read, intergenic_down10kb_read*1000.0/(intergenic_down10kb_base+1)))
	print("=====================================================================")
if __name__ == '__main__':
	main()
Traceback (most recent call last):
  File "<string>", line 1, in <module>
ModuleNotFoundError: No module named 'RSeQC'
